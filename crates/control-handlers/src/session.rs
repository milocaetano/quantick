//! Recorded-session playback and paper-trading projections.
//!
//! Two scopes, one owner module: what the trader is *replaying*, and what the
//! simulator holds while they do. Both read state the application already
//! maintains; neither recomputes anything inside a capture.
//!
//! The window hands each tab over through [`SessionTab`]: its paper desk as
//! the trade family already reads it, and — while a recording is linked —
//! the replay as [`ReplayRead`]. Everything the two scopes decide from those
//! is here.

pub use quantick_control_schema::session::*;

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::{CONTROL_SNAPSHOT_MAX_CLOSED_TRADES, CONTROL_SNAPSHOT_MAX_WORKING_ORDERS},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_host::{
    projection::{CaptureContext, ProjectionRegistryError},
    wire::{canonical_decimal, canonical_f32, unavailable, wire_usize},
};
use quantick_paper::{format::PositionSummary, risk_sizing::RiskState};
use quantick_replay::Session;
use quantick_sim::Order;

use crate::dock::ProjectionDock;
use crate::trade::TradeDesk;

/// One linked recording as the replay scope reads it: the session file, and
/// the live playback state.
pub trait ReplayRead {
    /// The recording being played.
    fn session(&self) -> &Session;
    fn playing(&self) -> bool;
    fn finished(&self) -> bool;
    fn position_ms(&self) -> i64;
    fn start_ms(&self) -> i64;
    fn end_ms(&self) -> i64;
    fn elapsed_ms(&self) -> i64;
    fn progress(&self) -> f32;
    fn speed(&self) -> f32;
    fn rewinds(&self) -> u64;
    /// `(played, total)` prints over the session's own day, from the same
    /// function the transport bar draws them with.
    fn day_prints(&self) -> (usize, usize);
    /// The name of the control trace that sits beside the recording.
    fn trace_file_name(&self) -> String;
}

/// One open tab as the session scopes read it.
pub trait SessionTab {
    /// The market the tab is on — what the ledger's rows belong to.
    fn symbol(&self) -> &str;
    /// The tab's paper desk.
    fn desk(&self) -> &dyn TradeDesk;
    /// The recording the tab is playing, if any.
    fn replay(&self) -> Option<&dyn ReplayRead>;
}

/// The window's tabs as the session scopes walk them.
pub trait SessionPort {
    /// Visit every tab in screen order.
    fn visit_session_tabs(&self, visit: &mut dyn FnMut(u64, &dyn SessionTab));
}

/// Dock the session module and its two scopes.
pub fn register<D, H>(registry: &mut D) -> Result<(), ProjectionRegistryError>
where
    D: ProjectionDock<H>,
    H: SessionPort + ?Sized + 'static,
{
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Session".to_owned(),
            description: "Recorded-session playback and the paper-trading ledger.".to_owned(),
        },
        revision::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(REPLAY_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Replay state",
        "Reports which recording each tab is playing, where the playhead is, and whether a control trace sits beside it.",
        &["observe", "observe.replay"],
        project_replay::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(PAPER_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Paper trading",
        "Reports the simulated position, the working orders and the closed-trade ledger, with its provenance declared.",
        &["observe", "observe.paper"],
        project_paper::<H>,
    )
}

/// Every tab's rows, built from one walk over the port.
fn per_tab<P, T>(app: &P, mut row: impl FnMut(u64, &dyn SessionTab) -> T) -> Vec<T>
where
    P: SessionPort + ?Sized,
{
    let mut rows = Vec::new();
    app.visit_session_tabs(&mut |tab_id, tab| rows.push(row(tab_id, tab)));
    rows
}

/// The module's revision key: what the trader changed, not what playback
/// moved on its own.
///
/// The playhead, the played count and the elapsed clock advance on every frame
/// of an active replay, and open profit re-marks on every print. A key holding
/// those would differ at every capture and so would mark nothing. What this
/// tracks is a deliberate change: which recording is loaded, whether it is
/// playing or finished, the speed, a seek, and the shape of the simulated
/// book — the same reasoning `health.rs` applies to its frame averages.
fn revision<P: SessionPort + ?Sized>(app: &P) -> Vec<SessionRevisionKey> {
    per_tab(app, |tab_id, tab| {
        let desk = tab.desk();
        SessionRevisionKey {
            tab_id,
            // The paper scope names the market its rows belong to, and a tab
            // can be pointed at another one without touching the ledger.
            symbol: tab.symbol().to_owned(),
            replay: tab.replay().map(|link| ReplayRevisionKey {
                file_name: file_name(&link.session().path),
                playing: link.playing(),
                finished: link.finished(),
                speed_milli: (link.speed() * 1000.0) as i64,
                rewinds: link.rewinds(),
            }),
            position: desk
                .position_summary()
                .map(|summary| (summary.side.as_str(), summary.quantity, summary.avg_price)),
            working_orders: desk
                .working_orders()
                .iter()
                .map(OrderRevisionKey::of)
                .collect(),
            closed_trades: desk.account().session_trades().len(),
            selected_trade_row: desk.selected_trade_index(),
        }
    })
}

/// The revision key's per-tab row. Its only contract is [`Eq`]: it is never
/// serialized and never leaves the registry.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionRevisionKey {
    tab_id: u64,
    symbol: String,
    replay: Option<ReplayRevisionKey>,
    position: Option<(&'static str, rust_decimal::Decimal, rust_decimal::Decimal)>,
    working_orders: Vec<OrderRevisionKey>,
    closed_trades: usize,
    selected_trade_row: Option<usize>,
}

/// One resting order as the revision key compares it. Every level the paper
/// scope publishes beside the order is here: a trader drags a stop or a target
/// without touching the order's own price, and a key that watched the price
/// alone would call that no change.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OrderRevisionKey {
    id: u64,
    price: Option<rust_decimal::Decimal>,
    quantity: rust_decimal::Decimal,
    stop_loss: Option<rust_decimal::Decimal>,
    take_profit: Option<rust_decimal::Decimal>,
    cancel_at: Option<rust_decimal::Decimal>,
}

impl OrderRevisionKey {
    fn of(order: &Order) -> Self {
        Self {
            id: order.id.0,
            price: order.price,
            quantity: order.quantity,
            stop_loss: order.bracket.stop_loss(),
            take_profit: order.bracket.take_profit(),
            cancel_at: order.cancel_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReplayRevisionKey {
    file_name: String,
    playing: bool,
    finished: bool,
    /// Speed is an `f32` the trader picks from a fixed set; comparing it in
    /// thousandths keeps the key `Eq` without pretending a float is exact.
    speed_milli: i64,
    rewinds: u64,
}

fn project_replay<P: SessionPort + ?Sized>(app: &P, _context: CaptureContext) -> ReplaySnapshot {
    ReplaySnapshot {
        tabs: per_tab(app, |tab_id, tab| TabReplaySnapshot {
            tab_id: WireU64::new(tab_id),
            replaying: tab.replay().is_some(),
            session: tab.replay().map(replay_session_snapshot),
        }),
    }
}

fn project_paper<P: SessionPort + ?Sized>(app: &P, _context: CaptureContext) -> PaperSnapshot {
    PaperSnapshot {
        tabs: per_tab(app, tab_paper_snapshot),
    }
}

fn replay_session_snapshot(link: &dyn ReplayRead) -> ReplaySessionSnapshot {
    let session = link.session();
    let joined = session.day_before_prints();
    let (day_played, day_total) = link.day_prints();
    ReplaySessionSnapshot {
        symbol: session.symbol.clone(),
        date: session
            .date
            .map(|date| format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)),
        file_name: file_name(&session.path),
        playing: link.playing(),
        finished: link.finished(),
        position_unix_ms: link.position_ms(),
        start_unix_ms: link.start_ms(),
        end_unix_ms: link.end_ms(),
        elapsed_ms: link.elapsed_ms(),
        // Both counted over the session's own day, from the same function
        // the transport bar draws them with. See the field docs.
        played_trades: wire_usize(day_played),
        total_trades: wire_usize(day_total),
        day_before_prints: wire_usize(joined),
        day_before: session.day_before_label(),
        day_before_problem: session
            .day_before_problem
            .as_ref()
            .map(|problem| format!("{} {}", problem.detail, problem.advice)),
        progress: canonical_f32(link.progress(), RATIO_DECIMAL_PLACES),
        speed: canonical_f32(link.speed(), RATIO_DECIMAL_PLACES),
        rewinds: WireU64::new(link.rewinds()),
        trace: ReplayTraceSnapshot {
            state: unavailable("trace_state_is_served_by_the_gateway_not_by_a_capture"),
            file_name: link.trace_file_name(),
        },
    }
}

fn tab_paper_snapshot(tab_id: u64, tab: &dyn SessionTab) -> TabPaperSnapshot {
    let desk = tab.desk();
    let account = desk.account();
    let orders = account.working_orders();
    let trades = account.session_trades();
    // The ledger is read from its end, so a truncated page keeps the newest
    // rows rather than the first ones ever recorded.
    let trade_page_start = trades
        .len()
        .saturating_sub(CONTROL_SNAPSHOT_MAX_CLOSED_TRADES);
    // Asked once and published from that one answer, so the state, the
    // size, the money and the sentence in this snapshot cannot describe
    // four different moments.
    let (risk_state, risk_blocks) = desk.risk_report();
    let risk_amount = match &risk_state {
        RiskState::Sized { risk, .. } | RiskState::OverBudget { risk, .. } => Some(risk.clone()),
        _ => None,
    };
    TabPaperSnapshot {
        tab_id: WireU64::new(tab_id),
        symbol: tab.symbol().to_owned(),
        provenance: PAPER_PROVENANCE.to_owned(),
        flat: account.is_flat(),
        armed_strategy: account
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone()),
        armed_strategy_refusal: account
            .selected_order_strategy()
            .and_then(|strategy| strategy.validate().err())
            .map(|error| error.advice().to_owned()),
        ruler_ticks: desk.ruler_ticks(),
        tick_size: canonical_decimal(account.tick_size()),
        risk_state: risk_state.code().to_owned(),
        risk_quantity: risk_state.derived_quantity().map(canonical_decimal),
        risk_amount: risk_amount
            .as_ref()
            .map(|risk| canonical_decimal(risk.amount)),
        risk_currency: risk_amount
            .as_ref()
            .map(|risk| risk.currency.code().to_owned()),
        risk_blocks_entry: risk_blocks,
        risk_sentence: Some(risk_state.sentence()).filter(|sentence| !sentence.is_empty()),
        position: account.position_summary().map(position_snapshot),
        working_orders: orders
            .iter()
            .take(CONTROL_SNAPSHOT_MAX_WORKING_ORDERS)
            .map(order_snapshot)
            .collect(),
        working_order_count: wire_usize(orders.len()),
        working_orders_truncated: orders.len() > CONTROL_SNAPSHOT_MAX_WORKING_ORDERS,
        closed_trades: trades[trade_page_start..]
            .iter()
            .map(closed_trade_snapshot)
            .collect(),
        closed_trade_count: wire_usize(trades.len()),
        closed_trades_truncated: trade_page_start > 0,
        closed_trades_page_start: wire_usize(trade_page_start),
        selected_trade_row: desk.selected_trade_index().map(wire_usize),
    }
}

fn position_snapshot(summary: PositionSummary) -> PaperPositionSnapshot {
    PaperPositionSnapshot {
        side: summary.side.as_str().to_owned(),
        quantity: canonical_decimal(summary.quantity),
        average_entry_price: canonical_decimal(summary.avg_price),
        open_points: summary.open_points.map(canonical_decimal),
    }
}
