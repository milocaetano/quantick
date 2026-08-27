//! Reviewed memory, protocol, and retention bounds for the control plane.

pub const CONTROL_TOKEN_BYTES: usize = 32;
pub const CONTROL_RUNTIME_ID_BYTES: usize = 16;
pub const CONTROL_HANDSHAKE_MAX_BYTES: usize = 64 * 1024;
pub const CONTROL_HANDSHAKE_MAX_SCOPES: usize = 32;
pub const CONTROL_REQUEST_ID_MAX_BYTES: usize = 128;
pub const CONTROL_DESCRIPTOR_MAX_BYTES: usize = 16 * 1024;
pub const CONTROL_DISCOVERY_MAX_ENTRIES: usize = 64;
pub const CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES: usize = 16 * 1024;
pub const CONTROL_PROTOCOL_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const CONTROL_MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const CONTROL_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const CONTROL_MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
pub const CONTROL_MAX_BUFFERED_RESPONSE_SLOTS: usize =
    CONTROL_MAX_BUFFERED_RESPONSE_BYTES / CONTROL_MAX_RESPONSE_BYTES;
pub const CONTROL_MAX_JSON_DEPTH: usize = 64;
pub const CONTROL_MAX_STRING_BYTES: usize = 256 * 1024;
pub const CONTROL_ID_MAX_BYTES: usize = 128;
pub const CONTROL_ID_MAX_SEGMENTS: usize = 8;
pub const CONTROL_CLIENT_NAME_MAX_BYTES: usize = 128;
pub const CONTROL_REASON_MAX_BYTES: usize = 1024;
pub const CONTROL_IDEMPOTENCY_KEY_MAX_BYTES: usize = 128;
pub const CONTROL_IDEMPOTENCY_MAX_ENTRIES: usize = 1_024;
pub const CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES: usize = 64 * 1024;
pub const CONTROL_IDEMPOTENCY_RETENTION_MS: u64 = 86_400_000;
pub const CONTROL_DEFAULT_PAGE_ITEMS: usize = 256;
pub const CONTROL_MAX_PAGE_ITEMS: usize = 2_048;
/// Maximum chart bars copied into an owned DTO on the application thread.
/// Calibrated independently from the larger off-thread protocol page ceiling.
pub const CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS: usize = 32;
pub const CONTROL_MAX_SNAPSHOT_SCOPES: usize = 32;

/// Book levels one side of one pane publishes in a capture. A host clips its
/// own ladder to the window the chart drew long before this; the limit is the
/// wire's own ceiling, so a host that ever widens that clip cannot widen the
/// page without saying so here.
pub const CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE: usize = 256;
/// Indicators one pane publishes in a capture. The chart itself caps visible
/// indicator panes far below this; the limit bounds a hostile or scripted
/// pane, not an arrangement a trader would build.
pub const CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE: usize = 64;
/// Drawings one pane publishes in a capture. A working chart carries tens of
/// marks; a page of this size is a whole session's annotation.
pub const CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE: usize = 512;
/// Working orders one paper-trading tab publishes in a capture. The simulator
/// holds no more than a trader can place by hand or a strategy can rest, so
/// this bounds a page rather than a plausible book.
pub const CONTROL_SNAPSHOT_MAX_WORKING_ORDERS: usize = 128;
/// Closed trades one paper-trading tab publishes in a capture. A session
/// ledger grows all day; a capture carries its newest page and says how many
/// rows it stands for.
///
/// Sized by measurement, not by taste. Each row crosses as eight exact
/// decimals, and every one of those is a `Decimal::normalize().to_string()`.
/// At 256 rows the paper scope alone cost 43 microseconds of a 250 microsecond
/// capture budget — more than every other scope put together — and a capture
/// of every scope exceeded the budget on a modest CI runner. At 64 it costs
/// about a quarter of that, still covers a full trading session, and the
/// `closed_trade_count` and `closed_trades_page_start` fields beside the page
/// keep a longer ledger honest. `observer_per_scope_capture_cost` is the test
/// that produced those numbers; rerun it before changing this.
pub const CONTROL_SNAPSHOT_MAX_CLOSED_TRADES: usize = 64;
pub const CONTROL_REQUEST_QUEUE_CAPACITY: usize = 64;
pub const CONTROL_MAX_CONNECTIONS: usize = 8;
pub const CONTROL_MAX_IN_FLIGHT_PER_CONNECTION: usize = 8;
pub const CONTROL_MAX_PARKED_WAITERS: usize = 16;
/// Parked `wait_for_change` registrations one connection may hold at once,
/// so a single client cannot take every slot from the others.
pub const CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION: usize = 4;
pub const CONTROL_HANDSHAKE_TIMEOUT_MS: u64 = 2_000;
pub const CONTROL_REQUEST_TIMEOUT_MS: u64 = 5_000;
pub const CONTROL_WAIT_TIMEOUT_MAX_MS: u64 = 30_000;
pub const CONTROL_CLIENT_RATE_PER_SECOND: u32 = 20;
pub const CONTROL_CLIENT_BURST: u32 = 40;
pub const CONTROL_NOTIFICATION_RATE_PER_MINUTE: u32 = 6;
pub const CONTROL_NOTIFICATION_BURST: u32 = 2;
/// Maximum control-plane work admitted in one application frame.
///
/// Calibrated by PR 2 at roughly nine times the measured 28 us p99 for one
/// coherent capture of every core observer scope. Later modules must remain
/// below this bound or paginate their payloads.
pub const CONTROL_UI_BUDGET_US: u64 = 250;
/// Deterministic admission ceiling even on machines where eight captures fit
/// inside the time budget. The elapsed-time guard remains authoritative too.
pub const CONTROL_UI_MAX_REQUESTS_PER_FRAME: usize = 4;
/// Controls one semantic-scene capture may carry.
///
/// Every registry behind the scene is fixed-size — the layer toggles, the
/// drawing tools, the dock's tabs — so the only unbounded contributor is the
/// trader's own tab strip. Set an order of magnitude above the roughly forty
/// controls a full window projects today, high enough that no real workspace
/// meets it and low enough that a runaway one cannot build an unbounded
/// payload on the application thread. A capture that meets it says so rather
/// than truncating in silence.
pub const CONTROL_SCENE_MAX_CONTROLS: usize = 512;
pub const CONTROL_EVENT_JOURNAL_CAPACITY: usize = 8_192;
pub const CONTROL_EVENT_MAX_BYTES: usize = 64 * 1024;
pub const CONTROL_EVENT_JOURNAL_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const CONTROL_AUDIT_MAX_ENTRIES: usize = 4_096;
pub const CONTROL_AUDIT_RECORD_MAX_BYTES: usize = 16 * 1024;
pub const CONTROL_AUDIT_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const CONTROL_AUDIT_RETENTION_MS: u64 = 86_400_000;
pub const CONTROL_EVIDENCE_MAX_BUNDLES: usize = 8;
pub const CONTROL_EVIDENCE_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const CONTROL_EVIDENCE_RETENTION_MS: u64 = 900_000;
/// The largest one retained bundle may encode to.
///
/// The store's own share of its total, so a single capture cannot evict every
/// other one to make room for itself. A bundle that would exceed it is refused
/// with `control.backpressure`, and a screenshot that would take it over
/// reports itself as not captured rather than costing the bundle its text.
pub const CONTROL_EVIDENCE_MAX_BUNDLE_BYTES: usize =
    CONTROL_EVIDENCE_MAX_TOTAL_BYTES / CONTROL_EVIDENCE_MAX_BUNDLES;
/// One chunk of a retained evidence resource, in raw bytes before transport
/// encoding.
///
/// Sized so a whole page of chunks, base64 expansion and envelope included,
/// stays well inside `CONTROL_MAX_RESPONSE_BYTES`, and so the ordered chunk
/// digests of a maximum-size bundle stay a short list rather than a payload of
/// their own.
pub const CONTROL_EVIDENCE_CHUNK_BYTES: usize = 512 * 1024;
/// Chunks one page of a retained evidence resource may carry.
///
/// The page bound that pairs with the chunk size: four chunks is two mebibytes
/// of payload, which leaves room for base64 expansion and the envelope inside
/// the response ceiling.
pub const CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE: usize = 4;
