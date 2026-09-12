//! Bounded command admission and owner-local diagnostics. Backlog means
//! accepted commands not yet admitted to a batch, not the instantaneous
//! channel length; parked means accepted by the owner and not yet in the
//! channel at all, because the channel was full ([`crate::worker_backlog`]).
//! One ticket is sampled until its admission is observed; unsampled tickets
//! and missed admission observations make waiting time or sample residence
//! unknown.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SendError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Instant;

use serde::Serialize;

#[cfg(test)]
use crate::worker_backlog::never;
use crate::worker_backlog::{Admitted, Merge, Parked};

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
    /// Commands the owner accepted that are waiting outside a full channel,
    /// right now. Zero inside the supported envelope.
    pub parked: u64,
    /// Commands that found the channel full when sent, cumulative: each was
    /// parked or folded into a parked one, never dropped.
    pub deferred: u64,
    /// Deferred commands that folded into the command parked before them,
    /// cumulative. Counted separately from the worker-side supersessions
    /// above because they happen before the channel, not in a batch.
    pub coalesced_parked: u64,
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
    parked: u64,
    deferred: u64,
    coalesced: u64,
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
///
/// The channel is bounded; a command the channel cannot take is parked here
/// and offered again by [`Self::pump`] and by every later send, so the owner
/// never blocks and never loses a command.
pub(crate) struct ObservedSender<T> {
    sender: SyncSender<T>,
    parked: RefCell<Parked<T>>,
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
                    parked: 0,
                    deferred: 0,
                    coalesced: 0,
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
    /// Bind a bounded channel whose commands never supersede one another: a
    /// full channel parks them in order. Both production workers have a
    /// superseding rule and use [`Self::bind_merging`].
    #[cfg(test)]
    pub(crate) fn bind<T>(self, sender: SyncSender<T>) -> ObservedSender<T> {
        self.bind_merging(sender, never)
    }
    /// Bind a bounded channel with the payload's own superseding rule, applied
    /// only while commands wait outside a full channel.
    pub(crate) fn bind_merging<T>(
        self,
        sender: SyncSender<T>,
        merge: Merge<T>,
    ) -> ObservedSender<T> {
        ObservedSender {
            sender,
            parked: RefCell::new(Parked::new(merge)),
            progress: self,
        }
    }
    /// What became of one offered command (`None` for a bare pump) and how
    /// many parked commands entered the channel on the way; every channel
    /// entry is an acceptance.
    fn record_admission(&self, admitted: Option<Admitted>, drained: usize, parked_now: usize) {
        for _ in 0..drained + usize::from(admitted == Some(Admitted::Queued)) {
            self.record_send(true);
        }
        let mut admission = self.observer.0.admission.get();
        match admitted {
            None | Some(Admitted::Queued) => {}
            Some(Admitted::Parked) => add(&mut admission.deferred, 1, &mut admission.valid),
            Some(Admitted::Merged) => {
                add(&mut admission.deferred, 1, &mut admission.valid);
                add(&mut admission.coalesced, 1, &mut admission.valid);
            }
        }
        admission.parked = parked_now as u64;
        self.observer.0.admission.set(admission);
    }
    /// The worker is gone: every command the disconnect discarded is a failed
    /// send, counted, and nothing is parked any more.
    fn record_lost(&self, lost: usize) {
        let mut admission = self.observer.0.admission.get();
        add(
            &mut admission.failed_sends,
            lost as u64,
            &mut admission.valid,
        );
        admission.parked = 0;
        self.observer.0.admission.set(admission);
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
    /// Offer one command. `Ok` means it is queued or parked — either way it
    /// will reach the worker; the caller never waits. `Err` means the worker
    /// is gone, and the command comes back.
    pub(crate) fn send(&self, command: T) -> Result<(), SendError<T>> {
        let mut parked = self.parked.borrow_mut();
        match parked.offer(&self.sender, command) {
            Ok((admitted, drained)) => {
                self.progress
                    .record_admission(Some(admitted), drained, parked.len());
                Ok(())
            }
            Err(refused) => {
                // Commands that entered the channel before the disconnect was
                // seen were accepted; the parked ones it discarded, plus this
                // one, are failed sends.
                self.progress.record_admission(None, refused.sent, 0);
                self.progress.record_lost(refused.lost + 1);
                Err(SendError(refused.command))
            }
        }
    }
    /// Offer parked commands to the channel again. Called from the owner's
    /// per-frame read, so a full channel drains at frame cadence even when
    /// nothing new is sent. Free when nothing is parked: one length read.
    pub(crate) fn pump(&self) {
        let mut parked = self.parked.borrow_mut();
        if parked.len() == 0 {
            return;
        }
        match parked.drain(&self.sender) {
            Ok(drained) => self.progress.record_admission(None, drained, parked.len()),
            Err(gone) => {
                self.progress.record_admission(None, gone.sent, 0);
                self.progress.record_lost(gone.lost);
            }
        }
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
        counts.parked = admission.parked;
        counts.deferred = admission.deferred;
        counts.coalesced_parked = admission.coalesced;
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
    armed: bool,
    pub inputs: usize,
    pub partials: usize,
    pub projects: usize,
}
impl<'a> Coalescing<'a> {
    pub(crate) fn new(progress: &'a SharedProgress) -> Self {
        Self {
            progress,
            armed: true,
            inputs: 0,
            partials: 0,
            projects: 0,
        }
    }

    pub(crate) fn publishing(mut self) {
        {
            let mut state = self.progress.lock();
            self.record(&mut state);
            state.phase = Phase::Publishing;
        }
        // The callback runs outside the ledger and may unwind. Its committed
        // subsets must not be recorded again by this guard's destructor.
        self.armed = false;
        self.progress.clock.phase(Phase::Publishing);
    }

    fn record(&self, state: &mut Ledger) {
        let Ledger { counts, valid, .. } = state;
        add(&mut counts.inputs_superseded, self.inputs as u64, valid);
        add(&mut counts.partials_superseded, self.partials as u64, valid);
        add(&mut counts.projects_superseded, self.projects as u64, valid);
    }
}
impl Drop for Coalescing<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.record(&mut self.progress.lock());
        }
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
