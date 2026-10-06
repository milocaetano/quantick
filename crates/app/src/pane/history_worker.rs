//! The UI's runtime adapter for the headless history publication owner.
use quantick_chart::history_publication::{
    HistoryPublication, HistoryRunner, HistoryRunnerStopped,
};
use quantick_chart::state::{ChartState, HistoryRebuild};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};

pub(super) type HistoryWorker = HistoryPublication<RecutThread>;
enum Job {
    Recut(HistoryRebuild, Arc<AtomicBool>),
    Retire(ChartState),
}
#[derive(Default)]
pub(super) struct RecutThread {
    channels: Option<(Sender<Job>, Receiver<ChartState>)>,
}
impl HistoryRunner for RecutThread {
    fn start(&mut self, rebuild: HistoryRebuild, cancelled: Arc<AtomicBool>) -> bool {
        let (jobs, _) = self.channels.get_or_insert_with(spawn);
        jobs.send(Job::Recut(rebuild, cancelled)).is_ok()
    }
    fn finished(&mut self) -> Result<Option<ChartState>, HistoryRunnerStopped> {
        let Some((_, results)) = &self.channels else {
            return Ok(None);
        };
        match results.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(HistoryRunnerStopped),
        }
    }
    fn retire(&mut self, state: ChartState) {
        if let Some((jobs, _)) = &self.channels {
            let _ = jobs.send(Job::Retire(state));
        }
    }
}

fn spawn() -> (Sender<Job>, Receiver<ChartState>) {
    let (jobs, inbox) = mpsc::channel();
    let (outbox, results) = mpsc::channel();
    std::thread::Builder::new().name("quantick-history-recut".to_owned()).spawn(move || {
        while let Ok(job) = inbox.recv() {
            match job {
                Job::Recut(rebuild, cancelled) => {
                    let started = std::time::Instant::now();
                    if let Some(result) = rebuild.run(|| cancelled.load(Ordering::Relaxed)) {
                        tracing::info!(target: "quantick::app", event_code = "HISTORY_RECUT_READY",
                            ms = started.elapsed().as_secs_f64() * 1000.0,
                            trades = result.trades().len(), "history recut finished beside the frame");
                        if outbox.send(result).is_err() { break; }
                    }
                }
                Job::Retire(state) => drop(state),
            }
        }
    }).expect("spawn history recut worker");
    (jobs, results)
}
