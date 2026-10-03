//! Lazy pane worker with a bounded source mailbox and one replaceable view request.
use quantick_orderflow::projection::flow_tape::{
    FlowChunk, FlowRequest, FlowRunner, FlowTapeFrame, FlowWorkerCache,
};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, SyncSender},
};
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) const QUEUE_CHUNKS: usize = 2;
// Flush a stalled partial without treating normal inter-frame packet gaps as idle.
const PARTIAL_IDLE_WAIT: Duration = Duration::from_millis(250);
enum Message {
    Chunk(FlowChunk),
    Wake,
}
struct Channels {
    input: SyncSender<Message>,
    request: Arc<Mutex<FlowRequest>>,
    output: Arc<Mutex<Option<Arc<FlowTapeFrame>>>>,
    thread: JoinHandle<()>,
}
#[derive(Default)]
pub(crate) struct FlowThread {
    channels: Option<Channels>,
}
impl FlowRunner for FlowThread {
    /// Returns true when a disconnected worker was reseeded and needs its source again.
    fn request(&mut self, request: &FlowRequest, notify: bool) -> bool {
        let stopped = self
            .channels
            .as_ref()
            .is_some_and(|channels| channels.thread.is_finished());
        if stopped {
            tracing::error!(target:"quantick::app", schema_version=1_u8,
                event_code="FLOW_EXECUTION_WORKER_DOWN", action="reseed_retained_source",
                "the FLOW execution worker stopped");
            self.channels = None;
        }
        let channels = self.channels.get_or_insert_with(|| spawn(request.clone()));
        *channels.request.lock().expect("FLOW view request") = request.clone();
        if notify {
            let _ = channels.input.try_send(Message::Wake);
        }
        stopped
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        let Some(channels) = &self.channels else {
            return Err(Box::new(chunk));
        };
        channels
            .input
            .try_send(Message::Chunk(chunk))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(Message::Chunk(chunk))
                | mpsc::TrySendError::Disconnected(Message::Chunk(chunk)) => Box::new(chunk),
                _ => unreachable!(),
            })
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        self.channels.as_ref()?.output.try_lock().ok()?.take()
    }
}
fn spawn(initial: FlowRequest) -> Channels {
    let (input, inbox) = mpsc::sync_channel(QUEUE_CHUNKS);
    let request = Arc::new(Mutex::new(initial));
    let output = Arc::new(Mutex::new(None));
    let worker_request = Arc::clone(&request);
    let worker_output = Arc::clone(&output);
    let thread = std::thread::Builder::new()
        .name("quantick-flow-executions".into())
        .spawn(move || {
            let mut cache = FlowWorkerCache::default();
            let mut pending = false;
            loop {
                let first = if pending {
                    match inbox.recv_timeout(PARTIAL_IDLE_WAIT) {
                        Ok(message) => Some(message),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match inbox.recv() {
                        Ok(message) => Some(message),
                        Err(_) => break,
                    }
                };
                let quiet = first.is_none();
                // Collect before reading the request: a packet cannot outrun its epoch.
                let messages = first
                    .into_iter()
                    .chain(inbox.try_iter().take(QUEUE_CHUNKS - 1))
                    .collect::<Vec<_>>();
                let request = worker_request.lock().expect("FLOW view request").clone();
                cache.select_request(&request);
                // Fold every packet; the headless gate schedules expensive regional projections.
                for message in messages {
                    if let Message::Chunk(chunk) = message {
                        cache.append(&chunk);
                    }
                }
                if let Some(frame) = cache.project_if_due(&request, quiet) {
                    let retired = worker_output.lock().expect("FLOW result").replace(frame);
                    drop(retired);
                }
                pending = cache.publication_pending(&request);
            }
        })
        .expect("spawn FLOW execution worker");
    Channels {
        input,
        request,
        output,
        thread,
    }
}

#[cfg(test)]
#[path = "tests/runner.rs"]
mod tests;
