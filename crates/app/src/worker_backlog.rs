//! What happens to a worker command when its bounded queue is full.
//!
//! The UI thread never blocks on a worker and never drops a command. A
//! command the queue cannot take right now is parked on the owner's side, in
//! order, and offered again on the next send and on the next per-frame read.
//! While it waits, a newer command that supersedes the last parked one folds
//! into it — a forming-bar update carries its predecessor's unsent prints, a
//! layout request replaces the layout it obsoletes — so a burst the worker is
//! behind on is queued as work, never as a longer history of the same work.
//! Both events are counted; [`crate::worker_progress`] publishes the counts.
//!
//! Which pairs fold is the command type's own knowledge, handed in as a plain
//! function: a payload with no superseding pairs uses [`never`], and the
//! buffer then only preserves order.

use std::collections::VecDeque;
use std::sync::mpsc::{SyncSender, TrySendError};

/// Whether `newer` supersedes `older`, the last command still parked.
///
/// Return `None` after folding `newer` into `older`; return `Some(newer)` when
/// both must reach the worker, in that order.
pub(crate) type Merge<T> = fn(older: &mut T, newer: T) -> Option<T>;

/// The policy for a payload with no superseding pairs: every command keeps
/// its place.
#[cfg(test)]
pub(crate) fn never<T>(_older: &mut T, newer: T) -> Option<T> {
    Some(newer)
}

/// What became of one offered command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admitted {
    /// It is in the worker's queue.
    Queued,
    /// The queue was full; it waits here, behind everything parked before it.
    Parked,
    /// The queue was full; it folded into the last parked command.
    Merged,
}

/// The worker is gone. Nothing parked can ever be delivered, so the buffer
/// was emptied; `lost` is how many commands that discarded, and `sent` how
/// many had entered the channel before the disconnect was seen — accepted,
/// and to be counted as such.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Disconnected {
    pub lost: usize,
    pub sent: usize,
}

/// The worker is gone and the offered command comes back to its owner,
/// together with the count of parked commands the disconnect discarded
/// (the returned command is not among them) and of those that entered the
/// channel before it was seen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refused<T> {
    pub command: T,
    pub lost: usize,
    pub sent: usize,
}

/// Commands accepted from the owner that the queue has not taken yet.
pub(crate) struct Parked<T> {
    commands: VecDeque<T>,
    merge: Merge<T>,
}

impl<T> Parked<T> {
    pub(crate) fn new(merge: Merge<T>) -> Self {
        Self {
            commands: VecDeque::new(),
            merge,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.commands.len()
    }

    /// Offer `command` to the queue, behind anything already parked.
    ///
    /// Returns what became of it and how many earlier parked commands entered
    /// the queue on the way.
    pub(crate) fn offer(
        &mut self,
        sender: &SyncSender<T>,
        command: T,
    ) -> Result<(Admitted, usize), Refused<T>> {
        let drained = match self.drain(sender) {
            Ok(drained) => drained,
            Err(gone) => {
                return Err(Refused {
                    command,
                    lost: gone.lost,
                    sent: gone.sent,
                });
            }
        };
        if !self.commands.is_empty() {
            return Ok((self.park(command), drained));
        }
        match sender.try_send(command) {
            Ok(()) => Ok((Admitted::Queued, drained)),
            Err(TrySendError::Full(command)) => Ok((self.park(command), drained)),
            Err(TrySendError::Disconnected(command)) => Err(Refused {
                command,
                lost: 0,
                sent: drained,
            }),
        }
    }

    fn park(&mut self, command: T) -> Admitted {
        match self.commands.back_mut() {
            Some(older) => match (self.merge)(older, command) {
                None => Admitted::Merged,
                Some(command) => {
                    self.commands.push_back(command);
                    Admitted::Parked
                }
            },
            None => {
                self.commands.push_back(command);
                Admitted::Parked
            }
        }
    }

    /// Move parked commands into the queue, oldest first, until it is full or
    /// this buffer is empty. Returns how many entered.
    pub(crate) fn drain(&mut self, sender: &SyncSender<T>) -> Result<usize, Disconnected> {
        let mut sent = 0;
        while let Some(command) = self.commands.pop_front() {
            match sender.try_send(command) {
                Ok(()) => sent += 1,
                Err(TrySendError::Full(command)) => {
                    self.commands.push_front(command);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    let lost = self.commands.len() + 1;
                    self.commands.clear();
                    return Err(Disconnected { lost, sent });
                }
            }
        }
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    /// Equal parities fold; the survivor sums, so a test can tell one merge
    /// from none.
    fn sum_equal_parity(older: &mut u64, newer: u64) -> Option<u64> {
        if *older % 2 == newer % 2 {
            *older += newer;
            None
        } else {
            Some(newer)
        }
    }

    #[test]
    fn a_full_queue_parks_in_order_and_drains_oldest_first() {
        let (tx, rx) = sync_channel(2);
        let mut parked = Parked::new(never);
        assert_eq!(parked.offer(&tx, 1).unwrap(), (Admitted::Queued, 0));
        assert_eq!(parked.offer(&tx, 2).unwrap(), (Admitted::Queued, 0));
        assert_eq!(parked.offer(&tx, 3).unwrap(), (Admitted::Parked, 0));
        assert_eq!(parked.offer(&tx, 4).unwrap(), (Admitted::Parked, 0));
        assert_eq!(parked.len(), 2);
        assert_eq!(rx.try_recv(), Ok(1));
        // One slot freed: the oldest parked command takes it, the new one
        // still waits behind the other.
        assert_eq!(parked.offer(&tx, 5).unwrap(), (Admitted::Parked, 1));
        assert_eq!(parked.len(), 2);
        assert_eq!(rx.try_recv(), Ok(2));
        assert_eq!(rx.try_recv(), Ok(3));
        assert_eq!(parked.drain(&tx).unwrap(), 2);
        assert_eq!(parked.len(), 0);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![4, 5]);
    }

    #[test]
    fn a_superseding_command_folds_into_the_last_parked_one_only() {
        let (tx, rx) = sync_channel(1);
        let mut parked = Parked::new(sum_equal_parity);
        assert_eq!(parked.offer(&tx, 1).unwrap().0, Admitted::Queued);
        assert_eq!(parked.offer(&tx, 2).unwrap().0, Admitted::Parked);
        assert_eq!(parked.offer(&tx, 4).unwrap().0, Admitted::Merged);
        // Odd after even: no fold, and no reaching past the back for a
        // partner — order is the point.
        assert_eq!(parked.offer(&tx, 3).unwrap().0, Admitted::Parked);
        assert_eq!(parked.offer(&tx, 8).unwrap().0, Admitted::Parked);
        assert_eq!(parked.len(), 3);
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(parked.drain(&tx).unwrap(), 1);
        assert_eq!(rx.try_recv(), Ok(6));
        assert_eq!(parked.drain(&tx).unwrap(), 1);
        assert_eq!(rx.try_recv(), Ok(3));
        assert_eq!(parked.drain(&tx).unwrap(), 1);
        assert_eq!(rx.try_recv(), Ok(8));
        assert_eq!(parked.len(), 0);
    }

    #[test]
    fn a_gone_worker_returns_the_command_and_counts_the_parked_ones_lost() {
        let (tx, rx) = sync_channel(1);
        let mut parked = Parked::new(never);
        assert_eq!(parked.offer(&tx, 1).unwrap().0, Admitted::Queued);
        assert_eq!(parked.offer(&tx, 2).unwrap().0, Admitted::Parked);
        assert_eq!(parked.offer(&tx, 3).unwrap().0, Admitted::Parked);
        drop(rx);
        assert_eq!(
            parked.offer(&tx, 4),
            Err(Refused {
                command: 4,
                lost: 2,
                sent: 0
            })
        );
        assert_eq!(parked.len(), 0);
        assert_eq!(
            parked.offer(&tx, 5),
            Err(Refused {
                command: 5,
                lost: 0,
                sent: 0
            })
        );
        assert_eq!(parked.drain(&tx), Ok(0));
    }
}
