//! The ledger's session layer, painted: the filter row, a session's row
//! inside a day, the inline rename and delete rows, and the tag strip.
//!
//! What a session is, which rows a filter keeps and what an edit does are
//! `quantick_paper::report::sessions` and `session_edit`; this file draws
//! them and turns a gesture into a [`JournalEdit`] the host applies — the same
//! edit the `session.journal.set` capability makes.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_paper::report::LoadedHistory;
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{
    LedgerFilter, LedgerQuery, SessionKey, kept_rows, session_labels, tag_counts,
};
use quantick_sim::history::{SessionSource, TradeTimes};
use rust_decimal::Decimal;

use super::{LEDGER_ROW_HEIGHT_PX, SourceFilter};
use crate::paper_calendar::{CalendarAction, CalendarState, DayIndex, draw_month, today};
use crate::paper_chrome::{fmt_signed_points, pill_toggle, points_color};
use crate::theme;
use crate::timezone::TzOffset;

/// Indent of a session's row under its day header.
const SESSION_INDENT_PX: f32 = 14.0;
/// Width of the ledger's search box.
const SEARCH_FIELD_PX: f32 = 120.0;
/// Most known tags the strip offers to complete.
const TAG_SUGGESTIONS: usize = 6;
/// One calendar cell of the ledger's date picker.
const PICKER_CELL_PX: egui::Vec2 = egui::vec2(30.0, 24.0);

/// What the ledger's session layer remembers between frames.
#[derive(Default)]
pub(crate) struct LedgerSessions {
    /// The filter row: source pills, a date span, the search box. Not
    /// persisted — the ledger opens on everything, as it always did.
    pub(super) filter: LedgerFilter,
    /// The date picker's month and whether it is open.
    calendar: CalendarState,
    /// Sessions folded shut inside their day.
    pub(super) collapsed: BTreeSet<SessionKey>,
    /// A saved row the trader selected — what the tag strip edits.
    pub(super) selected_saved: Option<(SessionKey, TradeTimes)>,
    /// The session row F2 and Del act on.
    pub(super) focused: Option<SessionKey>,
    /// A rename or a delete waiting on its inline row.
    pub(super) edit: Option<LedgerEdit>,
    /// The tag strip's text and the trade it was opened for.
    tag_text: Option<(SessionKey, TradeTimes, String)>,
    /// The filter's cut, made once per filter, reload or timezone.
    pub(super) cut: Option<LedgerCut>,
    /// The picker's day tints, built when it first opens after a reload.
    days: Option<DayIndex>,
    /// Every tag on record with its count, for the strip's completions.
    known_tags: Vec<(String, usize)>,
}

/// The saved rows a filter keeps and every session's label, in one timezone.
pub(super) struct LedgerCut {
    tz: TzOffset,
    /// Positions in the history's rows, oldest first.
    pub(super) kept: Vec<usize>,
    pub(super) labels: BTreeMap<SessionKey, String>,
}

/// A session edit waiting on its inline row.
pub(super) enum LedgerEdit {
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
    pub(super) fn session(&self) -> &SessionKey {
        match self {
            Self::Rename { session, .. } | Self::ConfirmDelete { session, .. } => session,
        }
    }
}

/// A session's row as the list paints it.
pub(super) struct SessionHead {
    pub(super) key: SessionKey,
    pub(super) label: String,
    pub(super) source: Option<SessionSource>,
    pub(super) count: usize,
    pub(super) net: Decimal,
    pub(super) folded: bool,
    pub(super) focused: bool,
    /// A tab is still journaling to it: delete is refused.
    pub(super) live: bool,
}

/// What a session row, its menu or an inline row asked for.
pub(super) enum SessionPress {
    Fold(SessionKey, bool),
    StartRename(SessionKey),
    StartDelete(SessionKey, String, usize),
    Cancel,
    Apply(JournalEdit),
}

impl LedgerSessions {
    /// The folder was read again: every cut and completion goes stale.
    pub(super) fn reloaded(&mut self, history: &LoadedHistory) {
        self.cut = None;
        self.days = None;
        self.known_tags = tag_counts(history);
    }

    /// Cut the saved rows for the filter in force, unless that cut stands.
    pub(super) fn ensure_cut(&mut self, history: Option<&LoadedHistory>, tz: TzOffset) {
        if self.cut.as_ref().is_some_and(|cut| cut.tz == tz) {
            return;
        }
        self.cut = Some(LedgerCut {
            tz,
            kept: history.map_or_else(Vec::new, |history| kept_rows(history, &self.filter, tz)),
            labels: history.map_or_else(BTreeMap::new, |history| session_labels(history, tz)),
        });
    }

    /// Apply what a session row or an inline row asked for; an edit leaves
    /// for the host.
    pub(super) fn apply(
        &mut self,
        press: SessionPress,
        labels: &dyn Fn(&SessionKey) -> String,
    ) -> Option<JournalEdit> {
        match press {
            SessionPress::Fold(key, folded) => {
                if folded {
                    self.collapsed.insert(key.clone());
                } else {
                    self.collapsed.remove(&key);
                }
                self.focused = Some(key);
                None
            }
            SessionPress::StartRename(key) => {
                self.focused = Some(key.clone());
                let text = labels(&key);
                self.edit = Some(LedgerEdit::Rename { session: key, text });
                None
            }
            SessionPress::StartDelete(key, label, trades) => {
                self.focused = Some(key.clone());
                self.edit = Some(LedgerEdit::ConfirmDelete {
                    session: key,
                    label,
                    trades,
                });
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

    /// Source pills, the date picker and the search box. Answers whether
    /// the filter moved.
    pub(super) fn draw_filter_row(
        &mut self,
        ui: &mut egui::Ui,
        history: Option<&LoadedHistory>,
        tz: TzOffset,
    ) -> bool {
        let before = self.filter.clone();
        ui.horizontal(|ui| {
            for pill in SourceFilter::PILLS {
                if pill_toggle(ui, pill.label(), self.filter.source == pill, pill.hover()).clicked()
                {
                    self.filter.source = pill;
                }
            }
            self.draw_date_button(ui, history, tz);
            ui.add(
                egui::TextEdit::singleline(&mut self.filter.search)
                    .id_salt("paper_ledger_search")
                    .hint_text("#tag or session")
                    .desired_width(SEARCH_FIELD_PX),
            )
            .on_hover_text(
                "#tags must all be on a trade; other words must all appear in its session's name",
            );
        });
        let moved = self.filter != before;
        if moved {
            self.cut = None;
        }
        moved
    }

    fn draw_date_button(
        &mut self,
        ui: &mut egui::Ui,
        history: Option<&LoadedHistory>,
        tz: TzOffset,
    ) {
        let label = match self.filter.range {
            Some(range) => format!("{} {}", icons::CALENDAR_BLANK, range.label()),
            None => format!("{} any date", icons::CALENDAR_BLANK),
        };
        let button = pill_toggle(
            ui,
            &label,
            self.filter.range.is_some(),
            "pick a day, or a second day for a span - tinted days hold trades",
        );
        let id = ui.id().with("paper_ledger_dates");
        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }
        if !ui.memory(|memory| memory.is_popup_open(id)) {
            return;
        }
        let days = self.days.get_or_insert_with(|| {
            history.map_or_else(DayIndex::default, |history| {
                DayIndex::build(history.rows.iter().map(|row| &row.trade), tz)
            })
        });
        let calendar = &mut self.calendar;
        let mut picked = None;
        egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                if draw_month(ui, days, calendar, PICKER_CELL_PX, today(tz))
                    == Some(CalendarAction::SelectionChanged)
                {
                    picked = Some(calendar.selection.range());
                }
            },
        );
        if let Some(range) = picked {
            self.filter.range = range;
        }
    }

    /// `N of M · K hidden [clear]`, pinned under the filter row while any
    /// filter cuts.
    pub(super) fn draw_filter_status(&mut self, ui: &mut egui::Ui, shown: usize, total: usize) {
        if self.filter.is_default() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{shown} of {total} · {} hidden",
                    total.saturating_sub(shown)
                ))
                .monospace()
                .small()
                .color(theme::TEXT_MUTED),
            );
            if ui
                .small_button("clear")
                .on_hover_text("drop every filter: both sources, any date, no search")
                .clicked()
            {
                self.filter = LedgerFilter::default();
                self.calendar.selection = Default::default();
                self.cut = None;
            }
        });
    }

    /// The strip above the totals: the selected trade's tags, editable, with
    /// the tags on record offered to complete. Enter or Save sends the edit.
    pub(super) fn draw_tag_strip(
        &mut self,
        ui: &mut egui::Ui,
        target: Option<(SessionKey, TradeTimes, &[String])>,
        focus: bool,
    ) -> Option<JournalEdit> {
        let Some((session, times, tags)) = target else {
            self.tag_text = None;
            return None;
        };
        if self
            .tag_text
            .as_ref()
            .is_none_or(|(key, at, _)| *key != session || *at != times)
        {
            let text = tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" ");
            self.tag_text = Some((session.clone(), times, text));
        }
        let (_, _, text) = self.tag_text.as_mut()?;
        let mut send = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("#")
                    .monospace()
                    .color(theme::TEXT_MUTED),
            );
            let field = ui.add(
                egui::TextEdit::singleline(text)
                    .id_salt("paper_ledger_tags")
                    .hint_text("#tag #tag")
                    .desired_width(ui.available_width() - 48.0),
            );
            if focus {
                field.request_focus();
            }
            send |= field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            send |= ui
                .small_button("Save")
                .on_hover_text("set this trade's tags - none clears them")
                .clicked();
        });
        let typed = LedgerQuery::parse(text).tags;
        ui.horizontal_wrapped(|ui| {
            for (tag, count) in self
                .known_tags
                .iter()
                .filter(|(tag, _)| !typed.contains(tag))
                .take(TAG_SUGGESTIONS)
            {
                if ui
                    .small_button(egui::RichText::new(format!("#{tag} {count}")).small())
                    .on_hover_text("add this tag")
                    .clicked()
                {
                    text.push_str(&format!(" #{tag}"));
                }
            }
        });
        send.then(|| JournalEdit::Tag {
            session,
            times,
            tags: text.split_whitespace().map(str::to_owned).collect(),
        })
    }
}

/// A session's row inside its day: fold caret, source glyph, label, count
/// and net. A click folds it; the menu renames or deletes it.
pub(super) fn draw_session_head(ui: &mut egui::Ui, head: &SessionHead) -> Option<SessionPress> {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::click(),
    );
    let mut press = None;
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if head.focused || response.hovered() {
            painter.rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
        }
        let mid = rect.center().y;
        let mut x = rect.left() + SESSION_INDENT_PX;
        let caret = if head.folded {
            icons::CARET_RIGHT
        } else {
            icons::CARET_DOWN
        };
        painter.text(
            egui::pos2(x, mid),
            egui::Align2::LEFT_CENTER,
            caret,
            egui::FontId::proportional(10.0),
            theme::TEXT_FAINT,
        );
        x += 14.0;
        if let Some((glyph, color)) = source_glyph(head.source) {
            painter.text(
                egui::pos2(x, mid),
                egui::Align2::LEFT_CENTER,
                glyph,
                egui::FontId::proportional(10.0),
                color,
            );
        }
        x += 14.0;
        painter.text(
            egui::pos2(x, mid),
            egui::Align2::LEFT_CENTER,
            &head.label,
            egui::FontId::monospace(10.0),
            theme::TEXT_PRIMARY,
        );
        painter.text(
            egui::pos2(rect.right() - 6.0, mid),
            egui::Align2::RIGHT_CENTER,
            format!("{} · {}", head.count, fmt_signed_points(head.net)),
            egui::FontId::monospace(10.0),
            points_color(head.net),
        );
    }
    if response.clicked() {
        press = Some(SessionPress::Fold(head.key.clone(), !head.folded));
    }
    let response = response.on_hover_text(format!(
        "{} · {} trade(s) - click to {}, right-click to rename or delete; F2 renames, Del deletes",
        head.key,
        head.count,
        if head.folded { "open it" } else { "fold it" }
    ));
    response.context_menu(|ui| {
        if let Some(menu) = session_menu(ui, &head.key, &head.label, head.count, head.live) {
            press = Some(menu);
        }
    });
    press
}

/// Rename and delete, as a session's or a trade's context menu offers them.
pub(super) fn session_menu(
    ui: &mut egui::Ui,
    key: &SessionKey,
    label: &str,
    trades: usize,
    live: bool,
) -> Option<SessionPress> {
    let mut press = None;
    if ui.button("Rename session…").clicked() {
        press = Some(SessionPress::StartRename(key.clone()));
        ui.close_menu();
    }
    let delete = ui
        .add_enabled(!live, egui::Button::new("Delete session…"))
        .on_disabled_hover_text("the session being traded now cannot be deleted");
    if delete.clicked() {
        press = Some(SessionPress::StartDelete(
            key.clone(),
            label.to_owned(),
            trades,
        ));
        ui.close_menu();
    }
    press
}

/// The inline row a rename or a delete puts where its session's row was.
pub(super) fn draw_inline(ui: &mut egui::Ui, edit: &mut LedgerEdit) -> Option<SessionPress> {
    let width = ui.available_width();
    let mut press = None;
    ui.allocate_ui_with_layout(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(LEDGER_ROW_HEIGHT_PX);
            ui.add_space(SESSION_INDENT_PX);
            match edit {
                LedgerEdit::Rename { session, text } => {
                    let field = ui.add(
                        egui::TextEdit::singleline(text)
                            .id_salt("paper_ledger_rename")
                            .hint_text("session name - empty clears it")
                            .desired_width(width - 90.0),
                    );
                    if !field.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
                        field.request_focus();
                    }
                    let enter =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                    if enter || ui.small_button("Save").clicked() {
                        press = Some(SessionPress::Apply(JournalEdit::Rename {
                            session: session.clone(),
                            name: text.clone(),
                        }));
                    } else if escape {
                        press = Some(SessionPress::Cancel);
                    }
                }
                LedgerEdit::ConfirmDelete {
                    session,
                    label,
                    trades,
                } => {
                    ui.label(
                        egui::RichText::new(format!("Delete \"{label}\"? {trades} trade(s)"))
                            .monospace()
                            .small()
                            .color(theme::WARN),
                    );
                    // Keep holds the focus: Enter is the safe answer.
                    let keep = ui.small_button("Keep");
                    if !keep.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
                        keep.request_focus();
                    }
                    if keep.clicked() || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                        press = Some(SessionPress::Cancel);
                    }
                    if ui
                        .small_button(egui::RichText::new("Delete session").color(theme::WARN))
                        .on_hover_text(
                            "moves the file to .deleted in the trades folder - recoverable by hand",
                        )
                        .clicked()
                    {
                        press = Some(SessionPress::Apply(JournalEdit::Delete {
                            session: session.clone(),
                        }));
                    }
                }
            }
        },
    );
    press
}

/// The line that says the current session is there, only filtered out.
pub(super) fn draw_hidden_live(ui: &mut egui::Ui, hidden: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        ui.painter().text(
            egui::pos2(rect.left() + SESSION_INDENT_PX, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("current session · {hidden} hidden by filter"),
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }
}

/// Replay wears an amber play glyph, live none, unrecorded a faint `?`.
pub(super) fn source_glyph(source: Option<SessionSource>) -> Option<(&'static str, egui::Color32)> {
    match source {
        Some(SessionSource::Replay) => Some((icons::PLAY, theme::AMBER)),
        Some(SessionSource::Live) => None,
        None => Some(("?", theme::TEXT_FAINT)),
    }
}
