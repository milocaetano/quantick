//! How an answer leaves the gateway: one whole frame per response, written
//! under the connection's writer lock, and, for a request whose ID is in
//! flight, that ID released under the same lock just before the frame goes
//! out (contract §5.2).

use std::io::Write as _;
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};

use quantick_control::codec::{BoundedCodec, FrameRole};
use quantick_control::id::RequestId;
use quantick_control::wire::{ResponseEnvelope, ResponseOutcome};

use super::ConnectionSlots;

/// Test build only: called on the answering thread once an answer that
/// released a request ID is written, with that ID and whether it was still
/// in flight just before the frame was written (it never should be). A test
/// holds the thread here, which is where a preempted thread used to sit with
/// the ID still in flight after its client had read the answer (#425).
#[cfg(test)]
pub(crate) type AnswerWritten = Arc<dyn Fn(&RequestId, bool) + Send + Sync>;

#[cfg(test)]
pub(crate) use test_gate::AnswerBeforeWrite;

/// A request ID this connection holds in flight (contract §5.2): a duplicate
/// is refused for as long as this lives. The one way to release it with an
/// answer is [`answer_and_release`]; dropped unanswered (a wait whose
/// connection closed) it releases the ID all the same, so no path can leave
/// an ID held for the rest of the connection.
pub(super) struct InFlightId {
    slots: Arc<ConnectionSlots>,
    request_id: RequestId,
}

impl InFlightId {
    pub(super) fn track(slots: &Arc<ConnectionSlots>, request_id: &RequestId) -> Self {
        slots.track(request_id);
        Self {
            slots: Arc::clone(slots),
            request_id: request_id.clone(),
        }
    }

    /// Take the ID out of a hand-over slot, if nobody took it yet.
    pub(super) fn take(slot: &Mutex<Option<Self>>) -> Option<Self> {
        slot.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

impl Drop for InFlightId {
    fn drop(&mut self) {
        self.slots.forget(&self.request_id);
    }
}

/// Write the terminal answer to a request, releasing its ID if it holds one.
///
/// The release happens under the writer lock, immediately before the frame
/// is written. A client learns that the ID is free only by reading this
/// answer, so a reuse sent after that read is never refused as a duplicate
/// (#425). A duplicate the reader admits between the release and the write
/// is answered after this answer, never before it: every answer on the
/// connection takes the same lock.
pub(super) fn answer_and_release(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    response: ResponseEnvelope,
    in_flight: Option<InFlightId>,
) {
    #[cfg(test)]
    let observed = in_flight
        .as_ref()
        .map(|held| (Arc::clone(&held.slots), held.request_id.clone()));
    #[cfg(test)]
    let mut in_flight_when_written = false;
    #[cfg(test)]
    test_gate::before_write(in_flight.as_ref());
    write_answer(
        writer,
        codec,
        response,
        move || drop(in_flight),
        || {
            #[cfg(test)]
            if let Some((slots, request_id)) = &observed {
                in_flight_when_written = slots.is_in_flight(request_id);
            }
        },
    );
    #[cfg(test)]
    if let Some((slots, request_id)) = &observed
        && let Some(written) = &slots.answer_written
    {
        written(request_id, in_flight_when_written);
    }
}

/// Write an answer that releases nothing: a refusal for a request this
/// connection never tracked.
pub(super) fn send_response(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    response: ResponseEnvelope,
) {
    write_answer(writer, codec, response, || {}, || {});
}

/// `release` runs under the writer lock, then `before_write`, then the
/// frame is written, still under the lock.
fn write_answer(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    mut response: ResponseEnvelope,
    release: impl FnOnce(),
    before_write: impl FnOnce(),
) {
    let frame = match codec.encode(FrameRole::Response, &response) {
        Ok(frame) => Some(frame),
        Err(error) => {
            response.capture_revision = None;
            response.module_revisions.clear();
            response.outcome = ResponseOutcome::Failure {
                error: super::super::encode_refusal::unencodable(&error),
            };
            codec.encode(FrameRole::Response, &response).ok()
        }
    };
    let mut stream = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    release();
    before_write();
    // An answer not even the refusal can encode writes nothing; its ID is
    // released all the same, since no answer will ever carry it. A write
    // that fails part-way has already put a truncated frame on the wire:
    // every byte after it would be read as that frame's payload. The
    // connection cannot be recovered, so it is closed rather than left
    // writing garbage the client will parse as answers.
    if let Some(frame) = frame
        && stream.write_all(&frame).is_err()
    {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod test_gate {
    use super::{Arc, InFlightId, RequestId};

    /// A bounded per-instance gate before the writer lock or response bytes.
    pub(crate) type AnswerBeforeWrite = Arc<dyn Fn(&RequestId) + Send + Sync>;

    pub(super) fn before_write(in_flight: Option<&InFlightId>) {
        if let Some(held) = in_flight
            && let Some(before_write) = &held.slots.answer_before_write
        {
            before_write(&held.request_id);
        }
    }
}
