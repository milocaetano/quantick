//! Bounded, owner-local diagnostics. Backlog means accepted commands not yet
//! admitted to a batch, not the instantaneous channel length. One ticket is
//! sampled until its admission is observed; unsampled tickets and missed
//! admission observations make waiting time or sample residence unknown.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SendError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Instant;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Idle,
    Applying,
    Publishing,
    Closed,
    Unwound,
}

/// Phase callbacks run without telemetry, channel or publication locks. The normal
/// clock is monotonic nanoseconds since owner creation; fixtures inject both
/// time and synchronization through this same production port.
pub(crate) trait ProgressClock: Send + Sync {
    /// Must be bounded and nonblocking; synchronization belongs in phase().
    fn now_ns(&self) -> Option<u64>;
    fn source(&self) -> &'static str {
        "monotonic"
    }
    fn phase(&self, _phase: Phase) {}
}

struct Monotonic(Instant);
impl ProgressClock for Monotonic {
    fn now_ns(&self) -> Option<u64> {
        self.0.elapsed().as_nanos().try_into().ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "ns")]
pub(crate) enum Age {
    Known(u64),
    Unknown,
    NotApplicable,
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Counts {
    pub accepted: u64,
    pub queued: u64,
    pub inflight: u64,
    pub retired: u64,
    /// Accepted work whose cycle did not finish; may already be partly applied.
    pub unfinished: u64,
    pub failed_sends: u64,
    pub cycles: u64,
    pub inputs_superseded: u64,
    pub partials_superseded: u64,
    pub projects_superseded: u64,
    pub output_attempts: u64,
    pub output_successes: u64,
    pub output_failures: u64,
    /// Replacing the BookPublished mailbox, not proof of UI adoption.
    pub mailbox_replacements: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ProgressSnapshot {
    pub instance: Option<u64>,
    pub clock_source: &'static str,
    pub phase: Phase,
    pub valid: bool,
    pub counts: Counts,
    pub sampled_ticket: Option<u64>,
    pub sample_age: Age,
    pub oldest_wait: Age,
    pub last_sampled_ticket: Option<u64>,
    pub last_sample_residence: Age,
    pub processing_age: Age,
    pub since_progress: Age,
}

#[derive(Clone, Copy)]
struct Sample {
    ticket: u64,
    at: Option<u64>,
}
#[derive(Clone, Copy)]
struct Admission {
    valid: bool,
    accepted: u64,
    failed_sends: u64,
}
struct SampleSlot {
    valid: bool,
    sample: Option<Sample>,
}
// Keep sample handoff and consumer progress on separate 64-byte boundaries.
// Storage is fixed per owner, independent of queue/history size.
#[repr(align(64))]
struct Sampling {
    pending: AtomicBool,
    slot: Mutex<SampleSlot>,
}
#[repr(align(64))]
struct Ledger {
    phase: Phase,
    valid: bool,
    counts: Counts,
    admitted: u64,
    last_sampled_ticket: Option<u64>,
    residence: Age,
    processing_at: Option<u64>,
    progress_at: Option<u64>,
}

/// Unique producer configuration, consumed when it is bound to a command sender.
/// Only its read-only observer can be cloned; both stay on the owner's thread.
pub(crate) struct WorkerProgress {
    observer: ProgressObserver,
}
struct OwnerProgress {
    admission: Cell<Admission>,
    shared: Arc<SharedProgress>,
}
/// Read-only owner-local observations outlive the command endpoint without
/// keeping its channel open. Rc prevents observation during a concurrent send.
#[derive(Clone)]
pub(crate) struct ProgressObserver(Rc<OwnerProgress>);

/// The only command endpoint for this owner. It is neither Clone nor Sync.
pub(crate) struct ObservedSender<T> {
    sender: Sender<T>,
    progress: WorkerProgress,
}

/// Only this consumer/sample state crosses into the worker thread. It cannot
/// report accepted counts without the local owner's observation.
pub(crate) struct SharedProgress {
    instance: Option<u64>,
    clock: Arc<dyn ProgressClock>,
    sampling: Sampling,
    ledger: Mutex<Ledger>,
}

fn add(value: &mut u64, amount: u64, valid: &mut bool) {
    if let Some(next) = value.checked_add(amount) {
        *value = next;
    } else {
        *valid = false;
    }
}
fn elapsed(now: Option<u64>, then: Option<u64>) -> Age {
    match (now, then) {
        (Some(now), Some(then)) => now.checked_sub(then).map_or(Age::Invalid, Age::Known),
        _ => Age::Unknown,
    }
}
fn subtract(total: u64, part: u64, valid: &mut bool) -> u64 {
    total.checked_sub(part).unwrap_or_else(|| {
        *valid = false;
        0
    })
}

impl WorkerProgress {
    pub(crate) fn new() -> Self {
        Self::with_clock(Arc::new(Monotonic(Instant::now())))
    }
    pub(crate) fn with_clock(clock: Arc<dyn ProgressClock>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let instance = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok();
        let shared = Arc::new(SharedProgress {
            instance,
            clock,
            sampling: Sampling {
                pending: AtomicBool::new(false),
                slot: Mutex::new(SampleSlot {
                    valid: true,
                    sample: None,
                }),
            },
            ledger: Mutex::new(Ledger {
                phase: Phase::Idle,
                valid: true,
                counts: Counts::default(),
                admitted: 0,
                last_sampled_ticket: None,
                residence: Age::NotApplicable,
                processing_at: None,
                progress_at: None,
            }),
        });
        Self {
            observer: ProgressObserver(Rc::new(OwnerProgress {
                admission: Cell::new(Admission {
                    valid: true,
                    accepted: 0,
                    failed_sends: 0,
                }),
                shared,
            })),
        }
    }
    pub(crate) fn observer(&self) -> &ProgressObserver {
        &self.observer
    }
    pub(crate) fn consumer(&self) -> Arc<SharedProgress> {
        Arc::clone(&self.observer.0.shared)
    }
    pub(crate) fn bind<T>(self, sender: Sender<T>) -> ObservedSender<T> {
        ObservedSender {
            sender,
            progress: self,
        }
    }
    fn record_send(&self, success: bool) {
        let mut admission = self.observer.0.admission.get();
        if success {
            add(&mut admission.accepted, 1, &mut admission.valid);
        } else {
            add(&mut admission.failed_sends, 1, &mut admission.valid);
        }
        self.observer.0.admission.set(admission);
        let shared = &self.observer.0.shared;
        // Unsampled commands update only local counters. The acquire observes
        // the consumer's acknowledgement before offering the next sample.
        if success && !shared.sampling.pending.load(Ordering::Acquire) {
            let mut slot = shared.lock_sample();
            if slot.sample.is_none() {
                slot.sample = Some(Sample {
                    ticket: admission.accepted,
                    at: shared.clock.now_ns(),
                });
                shared.sampling.pending.store(true, Ordering::Release);
            }
        }
    }
}

impl<T> ObservedSender<T> {
    pub(crate) fn send(&self, command: T) -> Result<(), SendError<T>> {
        let result = self.sender.send(command);
        self.progress.record_send(result.is_ok());
        result
    }
    pub(crate) fn snapshot(&self) -> ProgressSnapshot {
        self.progress.observer().snapshot()
    }
}
impl ProgressObserver {
    pub(crate) fn snapshot(&self) -> ProgressSnapshot {
        self.0.snapshot()
    }
}
impl OwnerProgress {
    fn snapshot(&self) -> ProgressSnapshot {
        // Same-owner observation cannot overlap enqueue/bookkeeping: the Rc
        // owner is not Send or Sync, and only the unique endpoint can send.
        self.shared.snapshot(self.admission.get())
    }
}

impl SharedProgress {
    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(|poison| {
            let mut state = poison.into_inner();
            state.valid = false;
            state
        })
    }
    fn lock_sample(&self) -> MutexGuard<'_, SampleSlot> {
        self.sampling.slot.lock().unwrap_or_else(|poison| {
            let mut state = poison.into_inner();
            state.valid = false;
            state
        })
    }
    fn try_sample(&self) -> Option<MutexGuard<'_, SampleSlot>> {
        match self.sampling.slot.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(poison)) => {
                let mut state = poison.into_inner();
                state.valid = false;
                Some(state)
            }
        }
    }
    pub(crate) fn begin(&self, count: usize) {
        {
            let mut state = self.lock();
            let mut sample_slot = self.try_sample();
            let now = self.clock.now_ns();
            let count = count as u64;
            let previous_admitted = state.admitted;
            let Ledger {
                counts,
                valid,
                admitted,
                ..
            } = &mut *state;
            counts.inflight = count;
            add(admitted, count, valid);
            // Never wait for a producer, including one between channel send and
            // bookkeeping. A missed transfer remains in the single sample slot;
            // a later batch/reader reports unknown residence, not a later time.
            if let Some(slot) = sample_slot.as_mut()
                && let Some(sample) = slot.sample
                && sample.ticket <= state.admitted
            {
                state.residence = if sample.ticket > previous_admitted {
                    elapsed(now, sample.at)
                } else {
                    Age::Unknown
                };
                state.last_sampled_ticket = Some(sample.ticket);
                slot.sample = None;
                self.sampling.pending.store(false, Ordering::Release);
            }
            state.processing_at = now;
            state.phase = Phase::Applying;
        }
        self.clock.phase(Phase::Applying);
    }
    pub(crate) fn superseded(&self, inputs: usize, partials: usize, projects: usize) {
        let mut state = self.lock();
        let Ledger { counts, valid, .. } = &mut *state;
        add(&mut counts.inputs_superseded, inputs as u64, valid);
        add(&mut counts.partials_superseded, partials as u64, valid);
        add(&mut counts.projects_superseded, projects as u64, valid);
    }
    pub(crate) fn publishing(&self) {
        self.lock().phase = Phase::Publishing;
        self.clock.phase(Phase::Publishing);
    }
    pub(crate) fn output(&self, success: bool) {
        let mut state = self.lock();
        let Ledger { counts, valid, .. } = &mut *state;
        add(&mut counts.output_attempts, 1, valid);
        add(
            if success {
                &mut counts.output_successes
            } else {
                &mut counts.output_failures
            },
            1,
            valid,
        );
    }
    pub(crate) fn finish(&self, mailbox: bool) {
        let now = self.clock.now_ns();
        {
            let mut state = self.lock();
            let Ledger { counts, valid, .. } = &mut *state;
            add(&mut counts.retired, counts.inflight, valid);
            counts.inflight = 0;
            add(&mut counts.cycles, 1, valid);
            add(&mut counts.mailbox_replacements, u64::from(mailbox), valid);
            state.processing_at = None;
            state.progress_at = now;
            state.phase = Phase::Idle;
        }
        self.clock.phase(Phase::Idle);
    }
    fn snapshot(&self, admission: Admission) -> ProgressSnapshot {
        // begin() only tries the sample lock while holding the ledger, so it
        // cannot invert this reader's lock order by waiting.
        let slot = self.lock_sample();
        let state = self.lock();
        let now = self.clock.now_ns();
        let terminal = matches!(state.phase, Phase::Closed | Phase::Unwound);
        let mut valid = state.valid && admission.valid && slot.valid;
        let mut counts = state.counts;
        counts.accepted = admission.accepted;
        counts.failed_sends = admission.failed_sends;
        if terminal {
            counts.unfinished = subtract(counts.accepted, counts.retired, &mut valid);
        } else {
            counts.queued = subtract(counts.accepted, state.admitted, &mut valid);
            valid &= counts.retired.checked_add(counts.inflight) == Some(state.admitted);
        }
        let pending = counts.queued != 0 || counts.inflight != 0;
        let sample = slot
            .sample
            .filter(|s| !terminal && s.ticket > state.admitted);
        let missed = slot.sample.filter(|s| s.ticket <= state.admitted);
        let sample_age = sample.map_or(Age::NotApplicable, |s| elapsed(now, s.at));
        let oldest_wait = if counts.queued == 0 {
            Age::NotApplicable
        } else if sample.is_some_and(|s| state.admitted.checked_add(1) == Some(s.ticket)) {
            sample_age
        } else {
            Age::Unknown
        };
        ProgressSnapshot {
            instance: self.instance,
            clock_source: self.clock.source(),
            phase: state.phase,
            valid,
            counts,
            sampled_ticket: sample.map(|s| s.ticket),
            sample_age,
            oldest_wait,
            last_sampled_ticket: missed.map(|s| s.ticket).or(state.last_sampled_ticket),
            last_sample_residence: if missed.is_some() {
                Age::Unknown
            } else {
                state.residence
            },
            processing_age: if state.counts.inflight == 0 {
                Age::NotApplicable
            } else {
                elapsed(now, state.processing_at)
            },
            since_progress: if pending {
                elapsed(now, state.progress_at)
            } else {
                Age::NotApplicable
            },
        }
    }
    pub(crate) fn lifecycle(self: &Arc<Self>) -> Lifecycle {
        Lifecycle(Arc::clone(self))
    }
}

pub(crate) struct Lifecycle(Arc<SharedProgress>);
impl Drop for Lifecycle {
    fn drop(&mut self) {
        let phase = if std::thread::panicking() {
            Phase::Unwound
        } else {
            Phase::Closed
        };
        {
            let mut state = self.0.lock();
            // A successful send may still be recording acceptance. The reader
            // joins both ledgers and derives all unretired work as unfinished.
            state.counts.inflight = 0;
            state.phase = phase;
            state.processing_at = None;
        }
        self.0.clock.phase(phase);
    }
}

/// Preserve the event protocol while recording delivery independently of cycles.
pub(crate) struct ObservedOutput<'a, T> {
    pub sender: &'a Sender<T>,
    pub progress: &'a SharedProgress,
}

/// Batch-local subsets survive unwinding after some domain work was applied.
/// One bounded ledger update per batch, including an incomplete batch.
pub(crate) struct Coalescing<'a> {
    progress: &'a SharedProgress,
    pub inputs: usize,
    pub partials: usize,
    pub projects: usize,
}
impl<'a> Coalescing<'a> {
    pub(crate) fn new(progress: &'a SharedProgress) -> Self {
        Self {
            progress,
            inputs: 0,
            partials: 0,
            projects: 0,
        }
    }
}
impl Drop for Coalescing<'_> {
    fn drop(&mut self) {
        self.progress
            .superseded(self.inputs, self.partials, self.projects);
    }
}
impl<T> ObservedOutput<'_, T> {
    pub(crate) fn send(&self, value: T) -> Result<(), SendError<T>> {
        let result = self.sender.send(value);
        self.progress.output(result.is_ok());
        result
    }
}

#[cfg(test)]
pub(crate) mod tests;
