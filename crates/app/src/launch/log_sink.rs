//! Where log lines leave the process: off the frame, through one writer
//! thread.
//!
//! Every `tracing` event used to write straight to stderr on the thread that
//! raised it — the UI thread included, several lines at a time when the
//! health summary falls due. A slow sink (a file on a disk a build is
//! hammering, a console window being drawn) then held the frame: the trader's
//! freeze dumps show the UI thread inside `NtWriteFile` under
//! `HealthSummary::warn_breaches` and `worker_diagnostics::emit_if_due`.
//!
//! Now a line is formatted where it is raised and queued; the writer thread
//! does the I/O. The queue is bounded, so a sink that never drains costs
//! memory once and nothing after: a line that finds it full is dropped and
//! counted, and the writer says how many went before its next line — a gap
//! in the log is announced, never silent.

use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

/// Lines the queue holds before new ones are dropped: minutes of the busiest
/// log this app writes, a few megabytes at worst.
pub(crate) const LOG_QUEUE_LINES: usize = 8_192;
/// How long an exit waits for queued lines to reach the sink.
const DRAIN_BUDGET: Duration = Duration::from_secs(2);

/// The process's sink, installed once by [`install`].
static SINK: OnceLock<LogSink> = OnceLock::new();

/// A handle onto the writer thread's queue; cheap to clone.
#[derive(Clone)]
pub(crate) struct LogSink {
    queue: SyncSender<Vec<u8>>,
    counts: Arc<Counts>,
}

#[derive(Default)]
struct Counts {
    /// Lines queued and not yet written.
    pending: AtomicUsize,
    /// Lines dropped since the writer last said so.
    dropped: AtomicU64,
}

/// One event's bytes, queued whole when the formatter lets go of it.
pub(crate) struct LogLine {
    sink: LogSink,
    bytes: Vec<u8>,
}

impl LogSink {
    /// A sink whose writer thread writes to `out`, in order.
    pub(crate) fn spawn(out: impl Write + Send + 'static, capacity: usize) -> Self {
        let (queue, lines) = sync_channel(capacity.max(1));
        let counts = Arc::new(Counts::default());
        let writer_counts = Arc::clone(&counts);
        std::thread::Builder::new()
            .name("quantick-log".into())
            .spawn(move || write_lines(out, &lines, &writer_counts))
            .expect("the log writer thread starts");
        Self { queue, counts }
    }

    /// Queue one line without waiting: a full queue drops it and counts it.
    fn send(&self, bytes: Vec<u8>) {
        self.counts.pending.fetch_add(1, Ordering::AcqRel);
        if let Err(error) = self.queue.try_send(bytes) {
            self.counts.pending.fetch_sub(1, Ordering::AcqRel);
            match error {
                TrySendError::Full(_) => {
                    self.counts.dropped.fetch_add(1, Ordering::AcqRel);
                }
                // The writer is gone (its sink failed): nothing left to do.
                TrySendError::Disconnected(_) => {}
            }
        }
    }

    /// Wait up to `budget` for every queued line to be written.
    pub(crate) fn drain(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        while self.counts.pending.load(Ordering::Acquire) > 0 {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogSink {
    type Writer = LogLine;

    fn make_writer(&'a self) -> Self::Writer {
        LogLine {
            sink: self.clone(),
            bytes: Vec::new(),
        }
    }
}

impl Write for LogLine {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LogLine {
    fn drop(&mut self) {
        if !self.bytes.is_empty() {
            self.sink.send(std::mem::take(&mut self.bytes));
        }
    }
}

/// The writer thread: each line in order, a drop notice before the line
/// that follows a gap. Ends when the sink fails or every sender is gone.
fn write_lines(mut out: impl Write, lines: &Receiver<Vec<u8>>, counts: &Counts) {
    for line in lines {
        let dropped = counts.dropped.swap(0, Ordering::AcqRel);
        let written = (dropped == 0
            || writeln!(
                out,
                "quantick: {dropped} log line(s) dropped: the log sink fell {LOG_QUEUE_LINES} lines behind"
            )
            .is_ok())
            && out.write_all(&line).is_ok();
        let _ = out.flush();
        counts.pending.fetch_sub(1, Ordering::AcqRel);
        if !written {
            return;
        }
    }
}

/// Install the process sink onto stderr and return the writer `tracing`
/// formats into.
pub(crate) fn install() -> LogSink {
    SINK.get_or_init(|| LogSink::spawn(std::io::stderr(), LOG_QUEUE_LINES))
        .clone()
}

/// Before the process ends: let what was logged reach the sink.
pub(crate) fn drain_before_exit() {
    if let Some(sink) = SINK.get() {
        let _ = sink.drain(DRAIN_BUDGET);
    }
}

#[cfg(test)]
mod log_sink_tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::channel;

    /// A sink that blocks until told to go on, then records what it got.
    struct Gate {
        open: Receiver<()>,
        got: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for Gate {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.open.recv();
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn line(sink: &LogSink, text: &str) {
        let mut writer = tracing_subscriber::fmt::MakeWriter::make_writer(sink);
        writer.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn a_stuck_sink_never_holds_the_caller_and_the_gap_is_announced() {
        let (open, gate) = channel();
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = LogSink::spawn(
            Gate {
                open: gate,
                got: Arc::clone(&got),
            },
            2,
        );
        let started = Instant::now();
        for n in 0..10 {
            line(&sink, &format!("line {n}\n"));
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "ten lines into a sink that writes nothing must not wait on it"
        );
        for _ in 0..64 {
            let _ = open.send(());
        }
        line(&sink, "after\n");
        for _ in 0..64 {
            let _ = open.send(());
        }
        assert!(sink.drain(Duration::from_secs(5)));
        let text = String::from_utf8(got.lock().unwrap().clone()).unwrap();
        let kept_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("line ")).collect();
        assert_eq!(
            kept_lines.first(),
            Some(&"line 0"),
            "the oldest line goes first: {text}"
        );
        assert!(text.contains("log line(s) dropped"), "{text}");
        assert!(text.ends_with("after\n"), "{text}");
        let kept = text.lines().filter(|l| l.starts_with("line ")).count();
        let dropped: usize = text
            .lines()
            .filter_map(|l| l.strip_prefix("quantick: "))
            .filter_map(|l| l.split(" ").next()?.parse::<usize>().ok())
            .sum();
        assert_eq!(
            kept + dropped,
            10,
            "every line is written or counted: {text}"
        );
    }

    #[test]
    fn lines_keep_their_order_and_drain_waits_for_them() {
        let got = Arc::new(Mutex::new(Vec::new()));
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = LogSink::spawn(Shared(Arc::clone(&got)), 64);
        for n in 0..50 {
            line(&sink, &format!("{n}\n"));
        }
        assert!(sink.drain(Duration::from_secs(5)));
        let text = String::from_utf8(got.lock().unwrap().clone()).unwrap();
        let expected: String = (0..50).map(|n| format!("{n}\n")).collect();
        assert_eq!(text, expected);
    }
}
