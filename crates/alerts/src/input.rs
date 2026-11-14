//! Facts injected by a consumer; no source, geometry or clock discovery.
use crate::spec::{DurableDrawingRef, MarketRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarIdentity {
    /// Changes on rebuild, reconnect, seek and source/aggregation replacement.
    pub epoch: u64,
    /// Authoritative sequence; timestamps do not uniquely identify bars.
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationKind {
    Trade,
    CommittedClose,
    /// Refresh availability/geometry without advancing price or cadence state.
    Lifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub coordinate: f64,
    pub price: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub first: Point,
    pub second: Point,
    pub extend_left: bool,
    pub extend_right: bool,
}

/// All geometry uses the same chart coordinate supplied in `AlertInput`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    Line(Line),
    Channel { line: Line, offset: f64 },
    Rectangle { first: Point, second: Point },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    SourceInactive,
    MissingDrawing,
    AmbiguousDrawing,
    LegacyIdentity,
    UnsupportedDrawing,
    OutsideTimeDomain,
    InvalidGeometry,
    Hydrating,
    ReplayPaused,
    Scaffold,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawingFact<'a> {
    pub target: &'a DurableDrawingRef,
    /// Geometry changes reseed crossing baselines.
    pub revision: u64,
    pub geometry: Result<Geometry, Unavailable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertInput<'a> {
    pub market: &'a MarketRef,
    pub utc_ms: i64,
    /// Monotonic within the supplied session epoch, not quote time.
    pub session_ms: u64,
    pub bar: BarIdentity,
    pub kind: ObservationKind,
    pub coordinate: f64,
    /// Actual observed trade or committed bar close. Never a quote.
    pub price: f64,
    pub availability: Result<(), Unavailable>,
    pub drawings: &'a [DrawingFact<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    Hydration,
    Reconnect,
    ReplaySeek,
    SourceChanged,
    AggregationChanged,
    GeometryChanged,
    WorkspaceChanged,
}
