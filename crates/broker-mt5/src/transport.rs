//! The real link: quantick listens on a loopback port and the execution
//! bridge dials it.
//!
//! The one part of this crate that owns threads and a socket, as the `feed`
//! level does for market data; everything it carries is decided in
//! [`crate::machine`]. Quantick listens and the bridge dials, never the other
//! way round: a local process that connects here can at worst show the venue
//! a false account, while a bridge that listened would take orders from any
//! local process. The port binds `127.0.0.1` only.
//!
//! One bridge at a time: while one is connected, a second waits in the
//! listen backlog until the first goes.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use crate::link::{BridgeLink, LinkEvent};

/// Longest line accepted from the bridge. A state line for one symbol is a
/// few kilobytes; the bound exists because a socket that accepts any local
/// process has to bound what it buffers. A longer line ends the connection.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

enum Outbound {
    Line(String),
    Stream(Option<TcpStream>),
}

/// The listening end of the execution channel.
pub struct TcpBridgeLink {
    inbound: Receiver<LinkEvent>,
    outbound: Sender<Outbound>,
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<TcpStream>>>,
    threads: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for TcpBridgeLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpBridgeLink")
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl TcpBridgeLink {
    /// Listen on `127.0.0.1:port` (0 asks the OS for a free port, which
    /// tests use). Binding happens here, so a port already taken is an
    /// error the caller sees rather than a silent thread.
    ///
    /// # Errors
    ///
    /// The bind's own error.
    pub fn listen(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let local_addr = listener.local_addr()?;
        let (inbound_tx, inbound) = mpsc::channel();
        let (outbound, outbound_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let writer = thread::Builder::new()
            .name("broker-mt5-writer".into())
            .spawn(move || write_loop(&outbound_rx))?;
        let acceptor = {
            let stop = Arc::clone(&stop);
            let current = Arc::clone(&current);
            let outbound = outbound.clone();
            thread::Builder::new()
                .name("broker-mt5-reader".into())
                .spawn(move || accept_loop(&listener, &inbound_tx, &outbound, &stop, &current))?
        };
        Ok(Self {
            inbound,
            outbound,
            local_addr,
            stop,
            current,
            threads: vec![writer, acceptor],
        })
    }

    /// The address bound, for a bridge to be pointed at.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl BridgeLink for TcpBridgeLink {
    fn send(&mut self, line: &str) {
        // The writer only ends when this link is dropped.
        let _ = self.outbound.send(Outbound::Line(line.to_owned()));
    }

    fn poll(&mut self, out: &mut Vec<LinkEvent>) {
        out.extend(self.inbound.try_iter());
    }
}

impl Drop for TcpBridgeLink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(stream) = self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
        // Wake a blocked `accept`; it sees the flag and returns.
        let _ = TcpStream::connect(self.local_addr);
        let _ = self.outbound.send(Outbound::Stream(None));
        let (closed, _) = mpsc::channel();
        self.outbound = closed;
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

fn accept_loop(
    listener: &TcpListener,
    inbound: &Sender<LinkEvent>,
    outbound: &Sender<Outbound>,
    stop: &AtomicBool,
    current: &Mutex<Option<TcpStream>>,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let _ = stream.set_nodelay(true);
        let (Ok(writer), Ok(closer)) = (stream.try_clone(), stream.try_clone()) else {
            continue;
        };
        *current.lock().unwrap_or_else(PoisonError::into_inner) = Some(closer);
        if outbound.send(Outbound::Stream(Some(writer))).is_err() {
            return;
        }
        read_lines(stream, inbound);
        let _ = outbound.send(Outbound::Stream(None));
        current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if inbound.send(LinkEvent::Disconnected).is_err() || stop.load(Ordering::SeqCst) {
            return;
        }
    }
}

/// Forward complete lines until the connection ends or breaks the bound.
fn read_lines(stream: TcpStream, inbound: &Sender<LinkEvent>) {
    let mut reader = BufReader::new(stream);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let limit = u64::try_from(MAX_LINE_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        match (&mut reader).take(limit).read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(_) if buffer.last() != Some(&b'\n') => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buffer);
        let line = text.trim();
        if !line.is_empty() && inbound.send(LinkEvent::Line(line.to_owned())).is_err() {
            return;
        }
    }
}

fn write_loop(outbound: &Receiver<Outbound>) {
    let mut stream: Option<TcpStream> = None;
    for message in outbound {
        match message {
            Outbound::Stream(next) => stream = next,
            Outbound::Line(line) => {
                let Some(open) = stream.as_mut() else {
                    continue;
                };
                let written = open
                    .write_all(line.as_bytes())
                    .and_then(|()| open.write_all(b"\n"))
                    .and_then(|()| open.flush());
                if written.is_err() {
                    stream = None;
                }
            }
        }
    }
}
