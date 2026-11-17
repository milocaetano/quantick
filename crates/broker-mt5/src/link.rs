//! The seam between the venue's decisions and the socket that carries them.

/// Something the link observed since it was last polled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// One complete line from the bridge, without its newline.
    Line(String),
    /// The bridge's connection closed (or failed). A new one may follow.
    Disconnected,
}

/// Carries request lines to the bridge and its lines back.
///
/// Two implementations: [`crate::transport::TcpBridgeLink`], the real socket,
/// and [`crate::fake::FakeBridge`], which tests and callers without a
/// terminal script by hand. Neither blocks: `send` queues and `poll` drains.
pub trait BridgeLink: Send {
    /// Queue one request line. A line sent while no bridge is connected is
    /// dropped; the venue resends what still matters when one says hello.
    fn send(&mut self, line: &str);

    /// Append everything observed since the last poll, in order.
    fn poll(&mut self, out: &mut Vec<LinkEvent>);

    /// The time the venue was handed, in Unix milliseconds. A real bridge
    /// keeps its own clock and its own heartbeat, so the socket ignores it;
    /// a bridge with no process behind it beats against this instead.
    fn clock(&mut self, _now_ms: i64) {}
}
