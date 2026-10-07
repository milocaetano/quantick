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

#[cfg(not(test))]
pub(super) type HistoryWorker = HistoryPublication<RecutThread>;

/// Hold the real worker's publication at a deterministic UI test boundary.
#[cfg(test)]
#[derive(Default)]
pub(super) struct HistoryWorker {
    publication: HistoryPublication<RecutThread>,
    pub held: bool,
    /// Reads as a failed rebuild until a retry, keeping every page.
    pub fail: bool,
}
#[cfg(test)]
impl HistoryWorker {
    pub fn poll(&mut self, state: &ChartState) -> bool {
        self.fail || (self.publication.poll(state) && !self.held)
    }
    pub fn pending(&self) -> bool {
        !self.fail && self.publication.pending()
    }
    pub fn failed(&self) -> bool {
        self.fail || self.publication.failed()
    }
    pub fn retry(&mut self) -> bool {
        std::mem::take(&mut self.fail) | self.publication.retry()
    }
}
#[cfg(test)]
impl std::ops::Deref for HistoryWorker {
    type Target = HistoryPublication<RecutThread>;
    fn deref(&self) -> &Self::Target {
        &self.publication
    }
}
#[cfg(test)]
impl std::ops::DerefMut for HistoryWorker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.publication
    }
}
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
        if self.channels.is_none() {
            self.channels = spawn();
        }
        self.channels
            .as_ref()
            .is_some_and(|(jobs, _)| jobs.send(Job::Recut(rebuild, cancelled)).is_ok())
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

fn spawn() -> Option<(Sender<Job>, Receiver<ChartState>)> {
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
    }).ok()?;
    Some((jobs, results))
}
