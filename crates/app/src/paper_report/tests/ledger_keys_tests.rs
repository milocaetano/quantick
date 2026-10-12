// The ledger frame by frame through egui: the inline rename saves on Enter
// and answers Esc only while it holds the keyboard, an edit whose row is
// gone is gone too, and the tag strip never widens the dock.

use eframe::egui;
use quantick_paper::report::ledger_state::SessionPress;
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{SessionKey, TradeRef};
use quantick_sim::history::{self, SessionSource};

use super::trade_at;
use crate::paper_calendar::CivilDate;
use crate::paper_report::LedgerAction;
use crate::paper_trading::PaperTrading;
use crate::scratch::ScratchDir;
use crate::timezone::TzOffset;

const UTC: TzOffset = TzOffset::new(0);
/// 2026-03-16 13:00:00 UTC.
const DAY_START: i64 = 1_773_666_000_000;
/// The panel the Trades tab draws in, sized like the dock's body.
const PANEL: &str = "ledger_keys_dock";

/// What one frame showed.
#[derive(Default)]
struct Frame {
    /// A widget held the keyboard before the panel drew: what the chart's
    /// drawing keys read at the top of the frame, and stand down for.
    keyboard_held: bool,
    action: Option<LedgerAction>,
    /// The panel's width once the frame laid it out.
    width: f32,
}

/// Two replays of one tape on one day, so the day heads each session.
fn ledger_with_two_sessions() -> (ScratchDir, PaperTrading) {
    let dir = ScratchDir::new("ledger-keys");
    for (file, closes) in [
        ("20260316-130000.csv", &[60_000, 120_000][..]),
        ("20260316-130000.rerun-1.csv", &[180_000][..]),
    ] {
        let mut text = history::write_header("WINJ26", SessionSource::Replay);
        for close in closes {
            text.push_str(&history::write_trade(&trade_at(DAY_START + close, 2)));
        }
        dir.write(format!("WINJ26/{file}"), &text);
    }
    let mut paper = PaperTrading::new();
    paper.account_mut().set_trades_dir(dir.path().to_path_buf());
    paper.set_symbol("WINJ26");
    (dir, paper)
}

fn session() -> SessionKey {
    SessionKey::parse("WINJ26/20260316-130000.csv").expect("a session key")
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(ctx: &egui::Context, paper: &mut PaperTrading, events: Vec<egui::Event>) -> Frame {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut shown = Frame::default();
    let _ = ctx.run(input, |ctx| {
        shown.keyboard_held = ctx.memory(|memory| memory.focused().is_some());
        egui::SidePanel::right(PANEL)
            .resizable(true)
            .default_width(320.0)
            .width_range(crate::dock::DOCK_MIN_WIDTH..=crate::dock::DOCK_MAX_WIDTH)
            .show(ctx, |ui| shown.action = paper.draw_trades_tab(ui, UTC));
        egui::CentralPanel::default().show(ctx, |_| {});
    });
    shown.width = egui::containers::panel::PanelState::load(ctx, egui::Id::new(PANEL))
        .map_or(0.0, |panel| panel.rect.width());
    shown
}

/// Start a rename or delete the way a key or a menu does, then draw the two
/// frames it takes the inline row to hold the keyboard through Esc.
fn start(ctx: &egui::Context, paper: &mut PaperTrading, press: SessionPress) {
    let report = paper.report_state_mut();
    let history = report.history_cache.as_ref();
    assert_eq!(report.sessions.state.apply(press, history), None);
    frame(ctx, paper, Vec::new());
    frame(ctx, paper, Vec::new());
}

fn editing(paper: &mut PaperTrading) -> bool {
    paper.report_state_mut().sessions.state.edit.is_some()
}

#[test]
fn the_inline_rename_saves_on_enter() {
    let (_dir, mut paper) = ledger_with_two_sessions();
    let ctx = egui::Context::default();
    frame(&ctx, &mut paper, Vec::new());
    start(&ctx, &mut paper, SessionPress::StartRename(session()));
    assert!(
        frame(&ctx, &mut paper, Vec::new()).keyboard_held,
        "the field took it"
    );
    frame(
        &ctx,
        &mut paper,
        vec![egui::Event::Text("Open drive".to_owned())],
    );
    let enter = frame(&ctx, &mut paper, vec![key(egui::Key::Enter)]);
    assert!(
        matches!(
            &enter.action,
            Some(LedgerAction::Edit(JournalEdit::Rename { session: named, name }))
                if *named == session() && name == "Open drive"
        ),
        "Enter saves the name typed"
    );
    assert!(!editing(&mut paper));
}

/// Esc in the focused rename is the rename's alone: the field keeps the
/// keyboard through the press, so the chart's drawing keys never hear it.
/// Clicked away from, a waiting rename leaves Esc to the chart.
#[test]
fn the_inline_rename_answers_esc_only_while_it_holds_the_keyboard() {
    let (_dir, mut paper) = ledger_with_two_sessions();
    let ctx = egui::Context::default();
    frame(&ctx, &mut paper, Vec::new());
    start(&ctx, &mut paper, SessionPress::StartRename(session()));
    let escape = frame(&ctx, &mut paper, vec![key(egui::Key::Escape)]);
    assert!(escape.keyboard_held, "the chart never heard this Esc");
    assert!(!editing(&mut paper), "Esc cancelled the rename");
    assert!(escape.action.is_none());

    start(&ctx, &mut paper, SessionPress::StartRename(session()));
    let away = egui::pos2(200.0, 400.0);
    for pressed in [true, false] {
        let click = egui::Event::PointerButton {
            pos: away,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            &mut paper,
            vec![egui::Event::PointerMoved(away), click],
        );
    }
    let escape = frame(&ctx, &mut paper, vec![key(egui::Key::Escape)]);
    assert!(
        !escape.keyboard_held,
        "nothing holds the keyboard: Esc is the chart's"
    );
    assert!(editing(&mut paper), "and the rename still waits");
}

/// A delete confirm whose day is folded shut has no row to answer from: it
/// is cancelled, and holds neither Esc nor the keyboard.
#[test]
fn an_edit_whose_row_is_gone_is_cancelled() {
    let (_dir, mut paper) = ledger_with_two_sessions();
    let ctx = egui::Context::default();
    frame(&ctx, &mut paper, Vec::new());
    let press = SessionPress::StartDelete(session(), "Replay".to_owned(), 2);
    start(&ctx, &mut paper, press);
    assert!(editing(&mut paper));
    paper
        .report_state_mut()
        .set_day_collapsed(CivilDate::from_ms(DAY_START, UTC), true);
    frame(&ctx, &mut paper, Vec::new());
    assert!(
        !editing(&mut paper),
        "a folded day leaves the confirm nowhere"
    );
    let after = frame(&ctx, &mut paper, Vec::new());
    assert!(!after.keyboard_held, "and its button lets the keyboard go");
}

/// Selecting a trade opens the tag strip; the strip fits the dock as it is
/// and never pushes it wider, frame after frame.
#[test]
fn selecting_a_trade_never_widens_the_dock() {
    let (_dir, mut paper) = ledger_with_two_sessions();
    let ctx = egui::Context::default();
    let before = (0..3)
        .map(|_| frame(&ctx, &mut paper, Vec::new()).width)
        .last()
        .unwrap_or_default();
    assert!(before > 0.0);
    let trade = trade_at(DAY_START + 60_000, 2);
    paper.report_state_mut().sessions.state.selected_saved = Some((
        session(),
        TradeRef {
            opened_ms: trade.opened_ms,
            closed_ms: trade.closed_ms,
            nth: 0,
        },
    ));
    for _ in 0..30 {
        let shown = frame(&ctx, &mut paper, Vec::new());
        assert_eq!(shown.width, before, "the dock kept its width");
    }
    assert!(
        paper.report_state_mut().sessions.state.tag_text.is_some(),
        "the tag strip is showing"
    );
}
