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
//! counted, and the count rides in front of the next line that gets in, so
//! the notice sits exactly where the gap is. A gap is announced, never
//! silent — at exit and on a panic too, which drain the queue first.
//!
//! It lives here because this is the level of the graph that owns threads;
//! the app hands `tracing` a closure over [`LogSink::line`].

use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

/// Lines the queue holds before new ones are dropped: minutes of the busiest
/// log this app writes, a few megabytes at worst.
pub const LOG_QUEUE_LINES: usize = 8_192;
/// How long an exit or a panic waits for queued lines to reach the sink.
const DRAIN_BUDGET: Duration = Duration::from_secs(2);

/// The process's sink, installed once by [`install`].
static SINK: OnceLock<LogSink> = OnceLock::new();

/// A handle onto the writer thread's queue; cheap to clone.
#[derive(Clone)]
pub struct LogSink {
    queue: SyncSender<Vec<u8>>,
    counts: Arc<Counts>,
}

struct Counts {
    capacity: usize,
    /// Lines queued and not yet written.
    pending: AtomicUsize,
    /// Lines dropped and not yet announced.
    dropped: AtomicU64,
}

/// One event's bytes, queued whole when the formatter lets go of it.
pub struct LogLine {
    sink: LogSink,
    bytes: Vec<u8>,
}

impl LogSink {
    /// A sink whose writer thread writes to `out`, in order.
    pub fn spawn(out: impl Write + Send + 'static, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (queue, lines) = sync_channel(capacity);
        let counts = Arc::new(Counts {
            capacity,
            pending: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        });
        let writer_counts = Arc::clone(&counts);
        std::thread::Builder::new()
            .name("quantick-log".into())
            .spawn(move || write_lines(out, &lines, &writer_counts))
            .expect("the log writer thread starts");
        Self { queue, counts }
    }

    /// A writer for one event: its bytes are queued when it is dropped.
    #[must_use]
    pub fn line(&self) -> LogLine {
        LogLine {
            sink: self.clone(),
            bytes: Vec::new(),
        }
    }

    /// Queue `bytes` without waiting, behind the notice of any gap before
    /// it. A full queue drops it, and the gap carries on to the next line.
    fn send(&self, bytes: Vec<u8>) {
        let gap = self.counts.dropped.swap(0, Ordering::AcqRel);
        let bytes = if gap == 0 {
            bytes
        } else {
            let mut announced = self.notice(gap);
            announced.extend_from_slice(&bytes);
            announced
        };
        self.counts.pending.fetch_add(1, Ordering::AcqRel);
        if let Err(error) = self.queue.try_send(bytes) {
            self.counts.pending.fetch_sub(1, Ordering::AcqRel);
            if let TrySendError::Full(_) = error {
                self.counts.dropped.fetch_add(gap + 1, Ordering::AcqRel);
            }
        }
    }

    fn notice(&self, gap: u64) -> Vec<u8> {
        format!(
            "quantick: {gap} log line(s) dropped: the log sink fell {} lines behind\n",
            self.counts.capacity
        )
        .into_bytes()
    }

    /// Wait up to `budget` for every queued line to be written, announcing
    /// a gap that no later line will carry.
    pub fn drain(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            if self.counts.pending.load(Ordering::Acquire) == 0 {
                let gap = self.counts.dropped.swap(0, Ordering::AcqRel);
                if gap == 0 {
                    return true;
                }
                self.send_notice(gap);
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn send_notice(&self, gap: u64) {
        self.counts.pending.fetch_add(1, Ordering::AcqRel);
        if self.queue.try_send(self.notice(gap)).is_err() {
            self.counts.pending.fetch_sub(1, Ordering::AcqRel);
            self.counts.dropped.fetch_add(gap, Ordering::AcqRel);
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

/// The writer thread: each line in order. A write that fails loses that
/// line and no other — the next one is tried as if nothing happened.
fn write_lines(mut out: impl Write, lines: &Receiver<Vec<u8>>, counts: &Counts) {
    for line in lines {
        let _ = out.write_all(&line);
        let _ = out.flush();
        counts.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Install the process sink onto stderr, once, and hand back its handle. A
/// panic drains it before the panic message, so the lines that led up to
/// the panic come out ahead of it.
pub fn install() -> LogSink {
    SINK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            drain_before_exit();
            previous(info);
        }));
        LogSink::spawn(std::io::stderr(), LOG_QUEUE_LINES)
    })
    .clone()
}

/// Before the process ends: let what was logged reach the sink.
pub fn drain_before_exit() {
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

    /// A sink that refuses its first write and takes every later one.
    struct FailsOnce {
        failed: bool,
        got: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for FailsOnce {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if !std::mem::replace(&mut self.failed, true) {
                return Err(std::io::Error::other("the sink hiccuped"));
            }
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn line(sink: &LogSink, text: &str) {
        sink.line().write_all(text.as_bytes()).unwrap();
    }

    fn text(got: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(got.lock().unwrap().clone()).unwrap()
    }

    fn dropped_in(text: &str) -> usize {
        text.lines()
            .filter_map(|l| l.strip_prefix("quantick: "))
            .filter_map(|l| l.split(' ').next()?.parse::<usize>().ok())
            .sum()
    }

    #[test]
    fn a_stuck_sink_never_holds_the_caller_and_the_gap_is_announced_where_it_is() {
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
        // Every write the gate lets through costs one token.
        for _ in 0..256 {
            let _ = open.send(());
        }
        // Wait out the backlog without announcing it: the next line is the
        // one that carries the gap.
        let deadline = Instant::now() + Duration::from_secs(5);
        while sink.counts.pending.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        line(&sink, "after\n");
        assert!(sink.drain(Duration::from_secs(5)));
        let text = text(&got);
        // Walk the output: each notice stands exactly where its gap is, so
        // after it the next line written is the one past the lines it counts.
        let mut next = 0;
        for line in text.lines() {
            if let Some(index) = line.strip_prefix("line ") {
                assert_eq!(index, next.to_string(), "a line out of place: {text}");
                next += 1;
            } else if line == "after" {
                assert_eq!(next, 10, "every line before it written or counted: {text}");
            } else {
                assert!(line.ends_with("the log sink fell 2 lines behind"), "{text}");
                next += dropped_in(line);
            }
        }
        assert!(
            text.ends_with("lines behind\nafter\n"),
            "a gap sits right before after: {text}"
        );
    }

    #[test]
    fn a_gap_no_later_line_carries_is_announced_on_drain() {
        let (open, gate) = channel();
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = LogSink::spawn(
            Gate {
                open: gate,
                got: Arc::clone(&got),
            },
            1,
        );
        for n in 0..5 {
            line(&sink, &format!("line {n}\n"));
        }
        for _ in 0..256 {
            let _ = open.send(());
        }
        assert!(sink.drain(Duration::from_secs(5)));
        let text = text(&got);
        let kept = text.lines().filter(|l| l.starts_with("line ")).count();
        assert!(text.ends_with("lines behind\n"), "{text}");
        assert_eq!(kept + dropped_in(&text), 5, "{text}");
    }

    #[test]
    fn a_failed_write_loses_that_line_and_no_other() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = LogSink::spawn(
            FailsOnce {
                failed: false,
                got: Arc::clone(&got),
            },
            8,
        );
        line(&sink, "lost\n");
        assert!(sink.drain(Duration::from_secs(5)));
        line(&sink, "kept\n");
        assert!(
            sink.drain(Duration::from_secs(5)),
            "nothing left pending behind the failure"
        );
        assert_eq!(text(&got), "kept\n");
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
        let expected: String = (0..50).map(|n| format!("{n}\n")).collect();
        assert_eq!(text(&got), expected);
    }
}
