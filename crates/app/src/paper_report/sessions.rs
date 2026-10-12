//! The ledger's session layer, painted: the filter row, a session's row
//! inside a day, the inline rename and delete rows, and the tag strip.
//!
//! What a session is, which rows a filter keeps, what an edit does and what
//! the layer remembers between frames are `quantick_paper::report::{sessions,
//! session_edit, ledger_state}`; this file draws them and turns a gesture
//! into a [`SessionPress`] — an edit leaves as the same [`JournalEdit`] the
//! `session.journal.set` capability makes.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_paper::report::LoadedHistory;
use quantick_paper::report::ledger_state::{
    LedgerEdit, LedgerState, LedgerView, LedgerViewChange, SessionPress, complete_tag,
    tag_suggestions,
};
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{LedgerFilter, SessionKey, TradeRef};
use quantick_sim::history::SessionSource;
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

/// What the ledger's session layer remembers between frames: the headless
/// state, and the date picker the window alone has.
#[derive(Default)]
pub(crate) struct LedgerSessions {
    pub(super) state: LedgerState,
    /// The date picker's month and whether it is open.
    calendar: CalendarState,
    /// The picker's day tints, built when it first opens after a reload.
    days: Option<DayIndex>,
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

impl LedgerSessions {
    /// The folder was read again.
    pub(super) fn reloaded(&mut self, history: &LoadedHistory) {
        self.state.reloaded(history);
        self.days = None;
    }

    /// Set the view from outside the filter row; a picked span the change
    /// replaced stops showing in the picker.
    pub(super) fn view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool) {
        if change.clear || change.range.is_some() {
            self.calendar.selection = Default::default();
        }
        let moved = self.state.view.apply(change);
        (self.state.view.clone(), moved)
    }

    /// Source pills, the date picker and the search box. The cut follows
    /// the filter by itself.
    pub(super) fn draw_filter_row(
        &mut self,
        ui: &mut egui::Ui,
        history: Option<&LoadedHistory>,
        tz: TzOffset,
    ) {
        ui.horizontal(|ui| {
            for pill in SourceFilter::PILLS {
                let on = self.state.view.filter.source == pill;
                if pill_toggle(ui, pill.label(), on, pill.hover()).clicked() {
                    self.state.view.filter.source = pill;
                }
            }
            self.draw_date_button(ui, history, tz);
            ui.add(
                egui::TextEdit::singleline(&mut self.state.view.filter.search)
                    .id_salt("paper_ledger_search")
                    .hint_text("#tag or session")
                    .desired_width(SEARCH_FIELD_PX),
            )
            .on_hover_text(
                "#tags must all be on a trade; other words must all appear in its session's name",
            );
        });
    }

    fn draw_date_button(
        &mut self,
        ui: &mut egui::Ui,
        history: Option<&LoadedHistory>,
        tz: TzOffset,
    ) {
        let label = match self.state.view.filter.range {
            Some(range) => format!("{} {}", icons::CALENDAR_BLANK, range.label()),
            None => format!("{} any date", icons::CALENDAR_BLANK),
        };
        let button = pill_toggle(
            ui,
            &label,
            self.state.view.filter.range.is_some(),
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
            self.state.view.filter.range = range;
        }
    }

    /// `N of M · K hidden [clear]`, pinned under the filter row while any
    /// filter cuts.
    pub(super) fn draw_filter_status(&mut self, ui: &mut egui::Ui, shown: usize, total: usize) {
        if self.state.view.filter.is_default() {
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
                self.state.view.filter = LedgerFilter::default();
                self.calendar.selection = Default::default();
            }
        });
    }

    /// The strip above the totals: the selected trade's tags, editable, with
    /// the tags on record offered to complete. Enter or Save sends the edit.
    pub(super) fn draw_tag_strip(
        &mut self,
        ui: &mut egui::Ui,
        target: Option<(SessionKey, TradeRef, &[String])>,
        focus: bool,
    ) -> Option<JournalEdit> {
        let Some((session, trade, tags)) = target else {
            self.state.tag_text = None;
            return None;
        };
        if self
            .state
            .tag_text
            .as_ref()
            .is_none_or(|(key, at, _)| *key != session || *at != trade)
        {
            let text = tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" ");
            self.state.tag_text = Some((session.clone(), trade, text));
        }
        let (_, _, text) = self.state.tag_text.as_mut()?;
        let mut send = false;
        let mut field_id = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("#")
                    .monospace()
                    .color(theme::TEXT_MUTED),
            );
            // Save first, from the right edge, and the field takes exactly
            // what is left: a row wider than the dock widens the dock.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                send |= ui
                    .small_button("Save")
                    .on_hover_text("set this trade's tags - none clears them")
                    .clicked();
                let field = ui.add(
                    egui::TextEdit::singleline(text)
                        .id_salt("paper_ledger_tags")
                        .hint_text("#tag #tag")
                        .desired_width(ui.available_width()),
                );
                if focus {
                    field.request_focus();
                }
                send |= field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                field_id = Some(field.id);
            });
        });
        let mut completed = false;
        ui.horizontal_wrapped(|ui| {
            for (tag, count) in tag_suggestions(&self.state.known_tags, text, TAG_SUGGESTIONS) {
                if ui
                    .small_button(egui::RichText::new(format!("#{tag} {count}")).small())
                    .on_hover_text("complete the tag being typed with this one")
                    .clicked()
                {
                    *text = complete_tag(text, tag);
                    completed = true;
                }
            }
        });
        // Back to the field with the caret after the completion, so the
        // next tag is typed straight on.
        if completed && let Some(id) = field_id {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            let end = egui::text::CCursor::new(text.chars().count());
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(end)));
            state.store(ui.ctx(), id);
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        send.then(|| JournalEdit::Tag {
            session,
            trade,
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
fn session_menu(
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
/// It takes the keyboard when the edit starts (`focus`, spent here) and
/// answers Esc only while its field or button still holds the keyboard;
/// clicked away from, it leaves Esc to the chart.
pub(super) fn draw_inline(
    ui: &mut egui::Ui,
    edit: &mut LedgerEdit,
    focus: &mut bool,
) -> Option<SessionPress> {
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
                    if std::mem::take(focus) {
                        field.request_focus();
                    }
                    hold_escape(ui, &field, true);
                    // Enter ends the field's focus as it commits; nothing
                    // asks for the focus again before this reads it.
                    let enter =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let escape = escape_pressed(ui, &field);
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
                    if std::mem::take(focus) {
                        keep.request_focus();
                    }
                    hold_escape(ui, &keep, false);
                    let escape = escape_pressed(ui, &keep);
                    if keep.clicked() || escape {
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

/// Keep the keyboard on `held` through Esc. egui drops the focus on Esc
/// before any panel draws, and the chart's Esc would then act on the same
/// press; held, the inline row answers it alone. `arrows` keeps the arrow
/// keys a text field's.
fn hold_escape(ui: &egui::Ui, held: &egui::Response, arrows: bool) {
    let filter = egui::EventFilter {
        escape: true,
        horizontal_arrows: arrows,
        vertical_arrows: arrows,
        tab: false,
    };
    ui.memory_mut(|memory| memory.set_focus_lock_filter(held.id, filter));
}

/// Esc, taken only while `held` has the keyboard; the focus goes with it.
fn escape_pressed(ui: &egui::Ui, held: &egui::Response) -> bool {
    let escape = held.has_focus()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if escape {
        held.surrender_focus();
    }
    escape
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
