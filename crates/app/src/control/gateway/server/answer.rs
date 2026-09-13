//! How an answer leaves the gateway: one whole frame per response, written
//! under the connection's writer lock, and, for a request whose ID is in
//! flight, that ID released under the same lock just before the frame goes
//! out (contract §5.2).

use std::io::Write as _;
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};

use quantick_control::codec::{BoundedCodec, FrameRole};
use quantick_control::wire::{ResponseEnvelope, ResponseOutcome};

use super::ConnectionSlots;

/// Test build only: called on the answering thread with the request ID an
/// answer released, once that answer is written. A test holds the thread
/// here, which is where a preempted thread used to sit with the ID still in
/// flight after its client had read the answer (#425).
#[cfg(test)]
pub(crate) type AnswerWritten = Arc<dyn Fn(&quantick_control::id::RequestId) + Send + Sync>;

/// Write the terminal answer to a request whose ID may be in flight on this
/// connection, releasing the ID.
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
    slots: &ConnectionSlots,
) {
    let request_id = response.request_id.clone();
    write_answer(writer, codec, response, || slots.forget(&request_id));
    #[cfg(test)]
    if let Some(written) = &slots.answer_written {
        written(&request_id);
    }
}

/// Write an answer that releases nothing: a refusal the reader sends for a
/// request it never tracked.
pub(super) fn send_response(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    response: ResponseEnvelope,
) {
    write_answer(writer, codec, response, || {});
}

fn write_answer(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    mut response: ResponseEnvelope,
    release: impl FnOnce(),
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
    // Not even the refusal encodes: nothing is written, and the ID is
    // released all the same, since no answer will ever carry it.
    let Some(frame) = frame else {
        return;
    };
    // A write that fails part-way has already put a truncated frame on the
    // wire: every byte after it would be read as that frame's payload. The
    // connection cannot be recovered, so it is closed rather than left
    // writing garbage the client will parse as answers.
    if stream.write_all(&frame).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
    }
}
