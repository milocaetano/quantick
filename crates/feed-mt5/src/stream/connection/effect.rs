//! Private input/effect vocabulary; no transport or clock handles.
use crate::protocol::{BridgeMsg, FeedMsg, ParseError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    AwaitHello,
    Connected,
    Closing,
    Closed,
}

pub(super) enum Input {
    Message(BridgeMsg),
    Blank { error: ParseError, snippet: String },
    Undecodable { error: ParseError, snippet: String },
    NonUtf8 { len: usize },
    Oversized,
    Eof,
    ReadError(String),
    Timeout,
    Request { count: u64, before_utc_ms: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PagerOp {
    IsInFlight,
    SettleOwed,
    Abandon,
}

pub(super) enum Effect {
    Wait(Phase),
    Publish,
    Live(quantick_engine::Trade),
    Write(FeedMsg),
    SampleTime,
    ReadCapture,
    Pager(PagerOp),
    Diagnostic,
    Finished,
}

pub(super) enum Ack {
    Published(bool),
    Written(Result<(), String>),
    Time(i64),
    Capture { enabled: bool, base_generation: u64 },
    Pager(bool),
    Diagnostic,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct InvalidInput;
