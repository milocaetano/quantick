//! The close that asks first: one question per tab holding unsaved workspace
//! edits, in strip order, before a tab leaves the strip or the app exits.
//!
//! The queue holds the request and its place in it, nothing else. Whether a
//! tab holds edits is the caller's question (the same one `tab.close` asks,
//! in `quantick_control_handlers::workspace_book`), and so is carrying out
//! each settled tab. Autosave on, or no tab holding edits, asks nothing: every
//! tab settles at once and the close goes ahead.

use std::collections::VecDeque;

/// What a close covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseScope {
    /// Tabs leaving the strip: each closes as soon as it is settled.
    Tabs,
    /// The app exiting: each tab is saved or let go where it stands, and the
    /// window closes once every tab is settled.
    App,
}

/// The trader's answer to one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAnswer {
    /// Write the tab's edits into its workspace, then close it.
    Save,
    /// Close it and let its edits go.
    CloseWithoutSaving,
    /// Close nothing more: the whole request ends here.
    Cancel,
}

impl CloseAnswer {
    /// The prompt's buttons, left to right; Esc is the first, Enter the last.
    pub const BUTTONS: [Self; 3] = [Self::Cancel, Self::CloseWithoutSaving, Self::Save];
}

/// What closing one tab would lose, as its question names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseQuestion {
    pub tab: u64,
    pub symbol: String,
    pub workspace: String,
    /// What changed since the workspace was saved.
    pub changes: Vec<String>,
    /// The clean tabs on the same workspace a save would also change.
    pub followers: Vec<String>,
}

/// What the caller does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStep {
    /// Settle this tab: `Some(true)` saves first, `Some(false)` lets its
    /// edits go, `None` means it holds none (an autosave write may be due).
    Settle {
        tab: u64,
        save: Option<bool>,
        scope: CloseScope,
    },
    /// A question is up: wait for [`CloseQueue::answer`].
    Ask,
    /// Every tab settled: the scope's close goes ahead.
    Done(CloseScope),
    /// No close in flight.
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    scope: CloseScope,
    tabs: VecDeque<u64>,
    asking: Option<CloseQuestion>,
    answered: Option<(u64, bool)>,
}

/// The close in flight, across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseQueue {
    request: Option<Request>,
    exit: bool,
}

impl CloseQueue {
    /// Start closing `tabs`, in order. A close already in flight keeps its
    /// place and this one is dropped: answers whether it started.
    pub fn request(&mut self, scope: CloseScope, tabs: impl IntoIterator<Item = u64>) -> bool {
        if self.request.is_some() {
            return false;
        }
        self.request = Some(Request {
            scope,
            tabs: tabs.into_iter().collect(),
            asking: None,
            answered: None,
        });
        true
    }

    /// The question waiting for an answer.
    #[must_use]
    pub fn question(&self) -> Option<&CloseQuestion> {
        self.request.as_ref()?.asking.as_ref()
    }

    /// Answer the question up; Cancel ends the request with nothing more
    /// closed. Without a question this does nothing.
    pub fn answer(&mut self, answer: CloseAnswer) {
        let Some(request) = self.request.as_mut() else {
            return;
        };
        let Some(asked) = request.asking.take() else {
            return;
        };
        match answer {
            CloseAnswer::Cancel => self.request = None,
            CloseAnswer::Save => request.answered = Some((asked.tab, true)),
            CloseAnswer::CloseWithoutSaving => request.answered = Some((asked.tab, false)),
        }
    }

    /// The next step: the answered tab first, then each tab in order, asking
    /// `pending` whether it holds edits when its turn comes.
    pub fn next(&mut self, pending: impl FnOnce(u64) -> Option<CloseQuestion>) -> CloseStep {
        let Some(request) = self.request.as_mut() else {
            return CloseStep::Idle;
        };
        let scope = request.scope;
        if request.asking.is_some() {
            return CloseStep::Ask;
        }
        if let Some((tab, save)) = request.answered.take() {
            let save = Some(save);
            return CloseStep::Settle { tab, save, scope };
        }
        let Some(tab) = request.tabs.pop_front() else {
            self.request = None;
            self.exit |= scope == CloseScope::App;
            return CloseStep::Done(scope);
        };
        match pending(tab) {
            Some(question) => {
                request.asking = Some(question);
                CloseStep::Ask
            }
            None => CloseStep::Settle {
                tab,
                save: None,
                scope,
            },
        }
    }

    /// End the request where it stands: a step failed.
    pub fn abort(&mut self) {
        self.request = None;
    }

    /// Whether an exit settled every tab since the last call: the window
    /// closes on `true`.
    pub fn take_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(tab: u64) -> CloseQuestion {
        CloseQuestion {
            tab,
            symbol: format!("T{tab}"),
            workspace: "Scalp".to_owned(),
            changes: vec!["RSI".to_owned()],
            followers: Vec::new(),
        }
    }

    /// Run the queue to its next wait, as the caller does, answering `held`
    /// tabs as pending. Answers the settled steps.
    fn drive(queue: &mut CloseQueue, held: &[u64]) -> (Vec<(u64, Option<bool>)>, CloseStep) {
        let mut settled = Vec::new();
        loop {
            match queue.next(|tab| held.contains(&tab).then(|| question(tab))) {
                CloseStep::Settle { tab, save, .. } => settled.push((tab, save)),
                stop => return (settled, stop),
            }
        }
    }

    #[test]
    fn a_single_tab_with_edits_asks_and_save_settles_it() {
        let mut queue = CloseQueue::default();
        assert!(queue.request(CloseScope::Tabs, [7]));
        assert_eq!(drive(&mut queue, &[7]), (vec![], CloseStep::Ask));
        assert_eq!(queue.question().map(|asked| asked.tab), Some(7));
        queue.answer(CloseAnswer::Save);
        let settled = drive(&mut queue, &[7]);
        assert_eq!(settled.0, vec![(7, Some(true))]);
        assert_eq!(settled.1, CloseStep::Done(CloseScope::Tabs));
        assert!(!queue.take_exit(), "a tab close never exits");
        assert_eq!(queue.next(|_| None), CloseStep::Idle);
    }

    #[test]
    fn a_window_asks_once_per_pending_tab_in_order() {
        let mut queue = CloseQueue::default();
        queue.request(CloseScope::Tabs, [1, 2, 3, 4]);
        let first = drive(&mut queue, &[2, 4]);
        assert_eq!(first, (vec![(1, None)], CloseStep::Ask));
        assert_eq!(queue.question().map(|asked| asked.tab), Some(2));
        queue.answer(CloseAnswer::CloseWithoutSaving);
        let second = drive(&mut queue, &[2, 4]);
        assert_eq!(second, (vec![(2, Some(false)), (3, None)], CloseStep::Ask));
        assert_eq!(queue.question().map(|asked| asked.tab), Some(4));
        queue.answer(CloseAnswer::Save);
        let last = drive(&mut queue, &[2, 4]);
        assert_eq!(last.0, vec![(4, Some(true))]);
        assert_eq!(last.1, CloseStep::Done(CloseScope::Tabs));
    }

    #[test]
    fn cancel_aborts_the_rest_of_the_request() {
        let mut queue = CloseQueue::default();
        queue.request(CloseScope::App, [1, 2, 3]);
        assert_eq!(drive(&mut queue, &[1, 2]).1, CloseStep::Ask);
        queue.answer(CloseAnswer::Save);
        assert_eq!(drive(&mut queue, &[1, 2]).0, vec![(1, Some(true))]);
        queue.answer(CloseAnswer::Cancel);
        assert_eq!(drive(&mut queue, &[1, 2]), (vec![], CloseStep::Idle));
        assert!(queue.question().is_none());
        assert!(!queue.take_exit(), "a cancelled exit keeps the app open");
        assert!(
            queue.request(CloseScope::Tabs, [3]),
            "the next close starts"
        );
    }

    #[test]
    fn nothing_pending_never_asks_and_an_exit_settles_every_tab() {
        let mut queue = CloseQueue::default();
        queue.request(CloseScope::App, [1, 2]);
        let all = drive(&mut queue, &[]);
        assert_eq!(all.0, vec![(1, None), (2, None)], "each tab flushes");
        assert_eq!(all.1, CloseStep::Done(CloseScope::App));
        assert!(queue.take_exit());
        assert!(!queue.take_exit(), "the exit is taken once");
    }

    #[test]
    fn a_second_close_waits_for_the_first() {
        let mut queue = CloseQueue::default();
        queue.request(CloseScope::Tabs, [1]);
        assert_eq!(drive(&mut queue, &[1]).1, CloseStep::Ask);
        assert!(!queue.request(CloseScope::App, [1, 2]));
        queue.answer(CloseAnswer::Save);
        queue.abort();
        assert_eq!(queue.next(|_| None), CloseStep::Idle);
    }
}
