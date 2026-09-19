//! Recorded-session playback and paper-trading projections.
//!
//! Two scopes, one owner module: what the trader is *replaying*, and what the
//! simulator holds while they do. Both read state the application already
//! maintains; neither recomputes anything inside a capture.

pub(crate) use quantick_control_schema::session::*;

use crate::app::TabsPort;
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::{CONTROL_SNAPSHOT_MAX_CLOSED_TRADES, CONTROL_SNAPSHOT_MAX_WORKING_ORDERS},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use quantick_sim::Order;

use crate::{paper_chrome::PositionSummary, tab::Tab};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    trace::ReplayTraceFile,
    types::{canonical_decimal, canonical_f32, unavailable, wire_usize},
};

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Session".to_owned(),
            description: "Recorded-session playback and the paper-trading ledger.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(REPLAY_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Replay state",
        "Reports which recording each tab is playing, where the playhead is, and whether a control trace sits beside it.",
        &["observe", "observe.replay"],
        project_replay,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(PAPER_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Paper trading",
        "Reports the simulated position, the working orders and the closed-trade ledger, with its provenance declared.",
        &["observe", "observe.paper"],
        project_paper,
    )
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
fn revision<P: TabsPort + ?Sized>(app: &P) -> Vec<SessionRevisionKey> {
    app.tab_reads()
        .tabs()
        .iter_with_ids()
        .map(|(tab_id, tab)| SessionRevisionKey {
            tab_id,
            // The paper scope names the market its rows belong to, and a tab
            // can be pointed at another one without touching the ledger.
            symbol: tab.symbol.clone(),
            replay: tab.replay.as_ref().map(|link| ReplayRevisionKey {
                file_name: file_name(&link.session.path),
                playing: link.status.is_playing(),
                finished: link.status.is_finished(),
                speed_milli: (link.status.speed() * 1000.0) as i64,
                rewinds: link.status.rewinds(),
            }),
            position: tab
                .paper
                .position_summary()
                .map(|summary| (summary.side.as_str(), summary.quantity, summary.avg_price)),
            working_orders: tab
                .paper
                .working_orders()
                .iter()
                .map(OrderRevisionKey::of)
                .collect(),
            closed_trades: tab.paper.account().session_trades().len(),
            selected_trade_row: tab.paper.account().selected_trade_index(),
        })
        .collect()
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

fn project_replay<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> ReplaySnapshot {
    replay_snapshot(app)
}

fn project_paper<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> PaperSnapshot {
    paper_snapshot(app)
}

fn replay_snapshot<P: TabsPort + ?Sized>(app: &P) -> ReplaySnapshot {
    ReplaySnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabReplaySnapshot {
                tab_id: WireU64::new(tab_id),
                replaying: tab.replay.is_some(),
                session: tab.replay.as_ref().map(replay_session_snapshot),
            })
            .collect(),
    }
}

fn replay_session_snapshot(link: &quantick_feed::replay::ReplayLink) -> ReplaySessionSnapshot {
    let status = &link.status;
    let joined = link.session.day_before_prints();
    let (day_played, day_total) = link.day_prints();
    ReplaySessionSnapshot {
        symbol: link.session.symbol.clone(),
        date: link
            .session
            .date
            .map(|date| format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)),
        file_name: file_name(&link.session.path),
        playing: status.is_playing(),
        finished: status.is_finished(),
        position_unix_ms: status.position_ms(),
        start_unix_ms: status.start_ms(),
        end_unix_ms: status.end_ms(),
        elapsed_ms: status.elapsed_ms(),
        // Both counted over the session's own day, from the same function
        // the transport bar draws them with. See the field docs.
        played_trades: wire_usize(day_played),
        total_trades: wire_usize(day_total),
        day_before_prints: wire_usize(joined),
        day_before: link.session.day_before_label(),
        day_before_problem: link
            .session
            .day_before_problem
            .as_ref()
            .map(|problem| format!("{} {}", problem.detail, problem.advice)),
        progress: canonical_f32(status.progress(), RATIO_DECIMAL_PLACES),
        speed: canonical_f32(status.speed(), RATIO_DECIMAL_PLACES),
        rewinds: WireU64::new(status.rewinds()),
        trace: ReplayTraceSnapshot {
            state: unavailable("trace_state_is_served_by_the_gateway_not_by_a_capture"),
            file_name: file_name(&ReplayTraceFile::path_for(&link.session.path)),
        },
    }
}

fn paper_snapshot<P: TabsPort + ?Sized>(app: &P) -> PaperSnapshot {
    PaperSnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(id, tab)| tab_paper_snapshot(id, tab))
            .collect(),
    }
}

fn tab_paper_snapshot(tab_id: u64, tab: &Tab) -> TabPaperSnapshot {
    let orders = tab.paper.account().working_orders();
    let trades = tab.paper.account().session_trades();
    // The ledger is read from its end, so a truncated page keeps the newest
    // rows rather than the first ones ever recorded.
    let trade_page_start = trades
        .len()
        .saturating_sub(CONTROL_SNAPSHOT_MAX_CLOSED_TRADES);
    // Asked once and published from that one answer, so the state, the
    // size, the money and the sentence in this snapshot cannot describe
    // four different moments.
    let (risk_state, risk_blocks) = tab.paper.risk_report();
    let risk_amount = match &risk_state {
        crate::risk_sizing::RiskState::Sized { risk, .. }
        | crate::risk_sizing::RiskState::OverBudget { risk, .. } => Some(risk.clone()),
        _ => None,
    };
    TabPaperSnapshot {
        tab_id: WireU64::new(tab_id),
        symbol: tab.symbol.clone(),
        provenance: PAPER_PROVENANCE.to_owned(),
        flat: tab.paper.account().is_flat(),
        armed_strategy: tab
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone()),
        armed_strategy_refusal: tab
            .paper
            .account()
            .selected_order_strategy()
            .and_then(|strategy| strategy.validate().err())
            .map(|error| error.advice().to_owned()),
        ruler_ticks: tab.paper.ruler_ticks(),
        tick_size: canonical_decimal(tab.paper.account().tick_size()),
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
        position: tab
            .paper
            .account()
            .position_summary()
            .map(position_snapshot),
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
        selected_trade_row: tab.paper.account().selected_trade_index().map(wire_usize),
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
