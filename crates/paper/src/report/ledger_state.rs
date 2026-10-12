//! What the Trades ledger keeps between frames, without its painting.
//!
//! The view — the filter row and the sessions folded shut — and the session
//! layer's gestures: the session row F2 and Del act on, the saved trade the
//! tag strip edits, a rename or delete waiting on its inline row, the strip's
//! text and the filter's cut of the saved rows. The window paints this state
//! and turns clicks and keys into [`SessionPress`]es; the control plane sets
//! and reads the view through [`LedgerView::apply`]. Every decision about it
//! lives here, a test away from a window.

use std::collections::{BTreeMap, BTreeSet};

use quantick_civil::{DateRange, TzOffset};
use quantick_sim::ClosedTrade;
use quantick_sim::history::{self, SessionSource};

use super::session_edit::JournalEdit;
use super::sessions::{
    JournaledTrade, LedgerFilter, SessionKey, SessionLabel, TradeRef, kept_rows, session_labels,
    tag_counts,
};
use super::{LoadedHistory, SourceFilter};

/// One of this session's closed trades as the ledger lists it: where it was
/// journaled, and the tags its file gives it.
pub struct LiveTrade<'a> {
    /// Its index among the session's closed trades.
    pub index: usize,
    pub trade: &'a ClosedTrade,
    /// `None` for a close no file under the trades folder holds.
    pub at: Option<&'a JournaledTrade>,
    pub source: Option<SessionSource>,
    pub tags: &'a [String],
}

/// What the ledger lists: its filter, and the sessions folded shut inside
/// their day. Not persisted — the ledger opens on everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerView {
    pub filter: LedgerFilter,
    pub folded: BTreeSet<SessionKey>,
}

/// One change to a [`LedgerView`]; what it leaves `None` stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerViewChange {
    /// Drop every filter first: both sources, any date, no search.
    pub clear: bool,
    pub source: Option<SourceFilter>,
    /// `Some(None)` drops the date span.
    pub range: Option<Option<DateRange>>,
    pub search: Option<String>,
    /// Sessions to fold (`true`) or open.
    pub fold: Vec<(SessionKey, bool)>,
}

impl LedgerView {
    /// Apply `change`; answers whether the view moved.
    pub fn apply(&mut self, change: &LedgerViewChange) -> bool {
        let before = self.clone();
        if change.clear {
            self.filter = LedgerFilter::default();
        }
        if let Some(source) = change.source {
            self.filter.source = source;
        }
        if let Some(range) = change.range {
            self.filter.range = range;
        }
        if let Some(search) = &change.search {
            self.filter.search.clone_from(search);
        }
        for (key, folded) in &change.fold {
            self.set_folded(key.clone(), *folded);
        }
        *self != before
    }

    pub fn set_folded(&mut self, key: SessionKey, folded: bool) {
        if folded {
            self.folded.insert(key);
        } else {
            self.folded.remove(&key);
        }
    }
}

/// A session edit waiting on its inline row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerEdit {
    Rename {
        session: SessionKey,
        text: String,
    },
    ConfirmDelete {
        session: SessionKey,
        label: String,
        trades: usize,
    },
}

impl LedgerEdit {
    #[must_use]
    pub fn session(&self) -> &SessionKey {
        match self {
            Self::Rename { session, .. } | Self::ConfirmDelete { session, .. } => session,
        }
    }
}

/// What a session row, its menu, an inline row or a key asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionPress {
    Fold(SessionKey, bool),
    StartRename(SessionKey),
    /// The session, its label and its trade count, which the confirm names.
    StartDelete(SessionKey, String, usize),
    Cancel,
    Apply(JournalEdit),
}

/// The saved rows a filter keeps and every session's label, for the filter
/// and timezone it was cut with.
pub struct LedgerCut {
    tz: TzOffset,
    filter: LedgerFilter,
    /// Positions in the history's rows, oldest first.
    pub kept: Vec<usize>,
    pub labels: BTreeMap<SessionKey, SessionLabel>,
}

/// The ledger's session layer between frames.
#[derive(Default)]
pub struct LedgerState {
    pub view: LedgerView,
    /// A saved row the trader selected — what the tag strip edits.
    pub selected_saved: Option<(SessionKey, TradeRef)>,
    /// The session row F2 and Del act on.
    pub focused: Option<SessionKey>,
    /// A rename or a delete waiting on its inline row.
    pub edit: Option<LedgerEdit>,
    /// The edit just started: its inline row takes the keyboard when next
    /// drawn, once. A trader who then clicks away keeps the edit and gets
    /// the keyboard back.
    pub focus_edit: bool,
    /// The tag strip's text and the trade it was opened for.
    pub tag_text: Option<(SessionKey, TradeRef, String)>,
    /// Every tag on record with its count, for the strip's completions.
    pub known_tags: Vec<(String, usize)>,
    /// The filter's cut; [`Self::ensure_cut`] keeps it current.
    pub cut: Option<LedgerCut>,
}

impl LedgerState {
    /// The folder was read again. The cut and the completions go stale, and
    /// a session the read no longer holds — deleted here, in another tab, by
    /// the control plane or by hand — leaves the focus, the selection, an
    /// open edit, the tag strip and the folds, so nothing acts on it again.
    pub fn reloaded(&mut self, history: &LoadedHistory) {
        self.cut = None;
        self.known_tags = tag_counts(history);
        let held = |key: &SessionKey| history.sessions.contains_key(key);
        self.view.folded.retain(held);
        if self.focused.as_ref().is_some_and(|key| !held(key)) {
            self.focused = None;
        }
        if self
            .selected_saved
            .as_ref()
            .is_some_and(|(key, _)| !held(key))
        {
            self.selected_saved = None;
        }
        if self.edit.as_ref().is_some_and(|edit| !held(edit.session())) {
            self.edit = None;
            self.focus_edit = false;
        }
        if self.tag_text.as_ref().is_some_and(|(key, ..)| !held(key)) {
            self.tag_text = None;
        }
    }

    /// Cut the saved rows for the filter in force, unless the standing cut
    /// was made for this filter in this timezone. The labels are built once
    /// per cut and serve the rows and the live trades alike.
    pub fn ensure_cut(&mut self, history: Option<&LoadedHistory>, tz: TzOffset) {
        if self
            .cut
            .as_ref()
            .is_some_and(|cut| cut.tz == tz && cut.filter == self.view.filter)
        {
            return;
        }
        let labels = history.map_or_else(BTreeMap::new, |history| session_labels(history, tz));
        let kept = history.map_or_else(Vec::new, |history| {
            kept_rows(history, &labels, &self.view.filter, tz)
        });
        self.cut = Some(LedgerCut {
            tz,
            filter: self.view.filter.clone(),
            kept,
            labels,
        });
    }

    /// Apply what a session row, an inline row or a key asked for; an edit
    /// leaves for the host. A rename starts from the name `history` holds.
    pub fn apply(
        &mut self,
        press: SessionPress,
        history: Option<&LoadedHistory>,
    ) -> Option<JournalEdit> {
        match press {
            SessionPress::Fold(key, folded) => {
                self.view.set_folded(key.clone(), folded);
                self.focused = Some(key);
                None
            }
            SessionPress::StartRename(key) => {
                self.focused = Some(key.clone());
                let text = history
                    .and_then(|history| history.sessions.get(&key))
                    .and_then(|info| info.name.clone())
                    .unwrap_or_default();
                self.edit = Some(LedgerEdit::Rename { session: key, text });
                self.focus_edit = true;
                None
            }
            SessionPress::StartDelete(key, label, trades) => {
                self.focused = Some(key.clone());
                self.edit = Some(LedgerEdit::ConfirmDelete {
                    session: key,
                    label,
                    trades,
                });
                self.focus_edit = true;
                None
            }
            SessionPress::Cancel => {
                self.edit = None;
                None
            }
            SessionPress::Apply(edit) => {
                self.edit = None;
                Some(edit)
            }
        }
    }

    /// A trade row was selected: F2 and Del no longer act on a session row
    /// picked before it, and never on the trade's own session — rename and
    /// delete belong to the session row.
    pub fn trade_selected(&mut self) {
        self.focused = None;
    }

    /// The rows were cut; `placed` says whether they hold the edit's inline
    /// row. An edit whose row is nowhere — its day folded, its session
    /// filtered away or past the revealed pages — is cancelled: nothing may
    /// wait on a row the trader cannot reach.
    pub fn edit_placed(&mut self, placed: bool) {
        if !placed {
            self.edit = None;
            self.focus_edit = false;
        }
    }

    /// This session's closed trades the filter in force keeps, newest
    /// first, and how many it hides — said on screen, never dropped. `at`
    /// and `sources` are index-aligned with `trades`; tags are read from
    /// each trade's own file, the label from the standing cut.
    #[must_use]
    pub fn live_trades<'a>(
        &self,
        trades: &'a [ClosedTrade],
        at: &'a [Option<JournaledTrade>],
        sources: &[SessionSource],
        history: Option<&'a LoadedHistory>,
        tz: TzOffset,
    ) -> (Vec<LiveTrade<'a>>, usize) {
        let matcher = self.view.filter.matcher(tz);
        let labels = self.cut.as_ref().map(|cut| &cut.labels);
        let mut hidden = 0;
        let mut kept = Vec::new();
        for (index, trade) in trades.iter().enumerate().rev() {
            let at = at.get(index).and_then(Option::as_ref);
            let source = sources.get(index).copied();
            let tags = at
                .and_then(|at| Some((history?.sessions.get(&at.session)?, at)))
                .map_or(&[][..], |(info, at)| info.tags_of(&at.trade));
            let label = at.and_then(|at| labels?.get(&at.session));
            if matcher.admits(source, trade, tags, label) {
                kept.push(LiveTrade {
                    index,
                    trade,
                    at,
                    source,
                    tags,
                });
            } else {
                hidden += 1;
            }
        }
        (kept, hidden)
    }

    /// The trade the tag strip edits, and its tags as its file holds them.
    /// `live` is the selected live trade's journal entry — `Some(None)` for
    /// a live trade no file holds, which has nothing to tag — and `None`
    /// leaves the saved selection to answer.
    #[must_use]
    pub fn tag_target(
        &self,
        live: Option<Option<&JournaledTrade>>,
        history: Option<&LoadedHistory>,
    ) -> Option<(SessionKey, TradeRef, Vec<String>)> {
        let (key, trade) = match live {
            Some(at) => {
                let at = at?;
                (at.session.clone(), at.trade)
            }
            None => self.selected_saved.clone()?,
        };
        let tags = history
            .and_then(|history| history.sessions.get(&key))
            .map(|info| info.tags_of(&trade).to_vec())
            .unwrap_or_default();
        Some((key, trade, tags))
    }

    /// What F2 (`rename`) or Del asks for. Both act on the focused session
    /// only — never on a selected trade's, since rename and delete belong to
    /// the session row; Del on a session in `live` asks for nothing — a tab
    /// is still writing it.
    #[must_use]
    pub fn key_press(
        &self,
        rename: bool,
        live: &BTreeSet<SessionKey>,
        history: Option<&LoadedHistory>,
    ) -> Option<SessionPress> {
        let key = self.focused.clone()?;
        if rename {
            return Some(SessionPress::StartRename(key));
        }
        if live.contains(&key) {
            return None;
        }
        let label = self
            .cut
            .as_ref()
            .and_then(|cut| cut.labels.get(&key))
            .map_or_else(|| key.to_string(), |label| label.text.clone());
        let trades = history
            .and_then(|history| history.sessions.get(&key))
            .map_or(0, |info| info.trades);
        Some(SessionPress::StartDelete(key, label, trades))
    }
}

/// The tags on record (`known`, most used first) that complete the tag
/// strip's `text`, at most `limit`: those starting with the token being
/// typed (the last one, unless the text ends in a space), none already typed.
#[must_use]
pub fn tag_suggestions<'k>(
    known: &'k [(String, usize)],
    text: &str,
    limit: usize,
) -> Vec<(&'k str, usize)> {
    let (typed, partial) = split_typing(text);
    let typed: Vec<String> = typed
        .split_whitespace()
        .filter_map(history::normalize_tag)
        .collect();
    let prefix = partial.trim_start_matches('#').to_lowercase();
    known
        .iter()
        .filter(|(tag, _)| tag.starts_with(&prefix) && !typed.contains(tag))
        .take(limit)
        .map(|(tag, count)| (tag.as_str(), *count))
        .collect()
}

/// The tag strip's `text` with the token being typed replaced by `#tag`
/// and a space, ready for the next one.
#[must_use]
pub fn complete_tag(text: &str, tag: &str) -> String {
    let (typed, _) = split_typing(text);
    if typed.trim().is_empty() {
        format!("#{tag} ")
    } else {
        format!("{} #{tag} ", typed.trim_end())
    }
}

/// `text` split before the token being typed: everything up to its last
/// whitespace, and what follows it — empty when the text ends in a space.
fn split_typing(text: &str) -> (&str, &str) {
    text.rsplit_once(char::is_whitespace).unwrap_or(("", text))
}

#[cfg(test)]
#[path = "ledger_state_tests.rs"]
mod ledger_state_tests;
