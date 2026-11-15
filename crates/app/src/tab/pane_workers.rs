//! The threads one pane's series is worked on beside the frame, keyed by the
//! pane's id on its tab: the history recut, delivering its rebuilt bars into
//! the tab session. The UI's runtime adapter for the headless history
//! publication owner lives here too.
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
pub(crate) type HistoryWorker = HistoryPublication<RecutThread>;

/// Hold the real worker's publication at a deterministic UI test boundary.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct HistoryWorker {
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
/// One pane's workers. The indicator worker joins it with #1042.
#[derive(Default)]
pub(crate) struct PaneWorkers {
    pub history: HistoryWorker,
}

enum Job {
    Recut(HistoryRebuild, Arc<AtomicBool>),
    Retire(ChartState),
}
#[derive(Default)]
pub(crate) struct RecutThread {
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

/// Every pane's workers on one tab, keyed by `ChartPane.id`.
pub(crate) type PaneWorkerBook = std::collections::BTreeMap<u64, PaneWorkers>;

const OPENED: &str = "a pane's series and workers open with the pane";

/// Give `pane` its series in the tab session and its workers on the tab.
pub(crate) fn open<'a>(
    session: &'a mut quantick_session::TabSession,
    workers: &'a mut PaneWorkerBook,
    pane: &crate::pane::ChartPane,
) -> PaneMutParts<'a> {
    let series = session.open_pane(pane.id, pane.spec.spec());
    let workers = workers.entry(pane.id).or_default();
    (series, workers)
}

/// A pane's series and workers, opened for it.
pub(crate) type PaneMutParts<'a> = (&'a mut quantick_session::PaneSeries, &'a mut PaneWorkers);

/// Drop pane `id`'s series and stop its workers, with the pane.
pub(crate) fn close(
    session: &mut quantick_session::TabSession,
    workers: &mut PaneWorkerBook,
    id: u64,
) {
    session.close_pane(id);
    if let Some(mut gone) = workers.remove(&id) {
        gone.history.cancel();
    }
}

/// `pane` beside the series and workers its tab keeps for it, read.
pub(crate) fn lend<'a>(
    pane: &'a crate::pane::ChartPane,
    session: &'a quantick_session::TabSession,
    workers: &'a PaneWorkerBook,
) -> crate::pane::PaneRef<'a> {
    crate::pane::PaneAt {
        pane,
        series: session.series(pane.id).expect(OPENED),
        workers: workers.get(&pane.id).expect(OPENED),
    }
}

/// See [`lend`], written.
pub(crate) fn lend_mut<'a>(
    pane: &'a mut crate::pane::ChartPane,
    session: &'a mut quantick_session::TabSession,
    workers: &'a mut PaneWorkerBook,
) -> crate::pane::PaneMut<'a> {
    let id = pane.id;
    crate::pane::PaneAt {
        pane,
        series: session.series_mut(id).expect(OPENED),
        workers: workers.get_mut(&id).expect(OPENED),
    }
}

/// Every pane in `panes` lent at once: the series and workers are borrowed
/// apart by id up front, on the stack, so a walk allocates nothing.
pub(crate) fn lend_all<'a>(
    panes: impl Iterator<Item = &'a mut crate::pane::ChartPane>,
    session: &'a mut quantick_session::TabSession,
    workers: &'a mut PaneWorkerBook,
) -> impl Iterator<Item = crate::pane::PaneMut<'a>> {
    type Book<T> = smallvec::SmallVec<[(u64, T); crate::canvas_layout::MAX_CANVAS_PANES]>;
    let mut series: Book<_> = session.series_by_id_mut().collect();
    let mut workers: Book<_> = workers.iter_mut().map(|(id, w)| (*id, w)).collect();
    panes.map(move |pane| {
        let id = pane.id;
        crate::pane::PaneAt {
            pane,
            series: take(&mut series, id),
            workers: take(&mut workers, id),
        }
    })
}

fn take<T, A: smallvec::Array<Item = (u64, T)>>(book: &mut smallvec::SmallVec<A>, id: u64) -> T {
    let at = book.iter().position(|(key, _)| *key == id).expect(OPENED);
    book.swap_remove(at).1
}
