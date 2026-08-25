//! Length-prefixed JSON framing with pre-allocation and structural limits.

use std::{fmt, io, io::Read, io::Write};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    handshake::{HandshakeReply, HandshakeRequest, ProtocolLimits},
    limits::{
        CONTROL_HANDSHAKE_MAX_BYTES, CONTROL_MAX_JSON_DEPTH, CONTROL_MAX_REQUEST_BYTES,
        CONTROL_MAX_RESPONSE_BYTES, CONTROL_MAX_STRING_BYTES, CONTROL_PROTOCOL_MAX_FRAME_BYTES,
    },
    wire::{RESERVED_ACTOR_FIELDS, RequestEnvelope, ResponseEnvelope},
};

const FRAME_HEADER_BYTES: usize = size_of::<u32>();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRole {
    Request,
    Response,
}

#[derive(Clone, Debug)]
pub struct BoundedCodec {
    max_frame_bytes: usize,
    max_request_bytes: usize,
    max_response_bytes: usize,
    max_json_depth: usize,
    max_string_bytes: usize,
}

impl Default for BoundedCodec {
    fn default() -> Self {
        Self {
            max_frame_bytes: CONTROL_PROTOCOL_MAX_FRAME_BYTES,
            max_request_bytes: CONTROL_MAX_REQUEST_BYTES,
            max_response_bytes: CONTROL_MAX_RESPONSE_BYTES,
            max_json_depth: CONTROL_MAX_JSON_DEPTH,
            max_string_bytes: CONTROL_MAX_STRING_BYTES,
        }
    }
}

impl BoundedCodec {
    /// Pre-authentication codec with a smaller allocation and parsing ceiling.
    #[must_use]
    pub const fn handshake() -> Self {
        Self {
            max_frame_bytes: CONTROL_HANDSHAKE_MAX_BYTES,
            max_request_bytes: CONTROL_HANDSHAKE_MAX_BYTES,
            max_response_bytes: CONTROL_HANDSHAKE_MAX_BYTES,
            max_json_depth: CONTROL_MAX_JSON_DEPTH,
            max_string_bytes: CONTROL_HANDSHAKE_MAX_BYTES,
        }
    }

    pub fn with_limits(
        max_frame_bytes: usize,
        max_request_bytes: usize,
        max_response_bytes: usize,
        max_json_depth: usize,
        max_string_bytes: usize,
    ) -> Result<Self, CodecError> {
        let valid = max_frame_bytes > 0
            && max_frame_bytes <= CONTROL_PROTOCOL_MAX_FRAME_BYTES
            && max_request_bytes > 0
            && max_request_bytes <= CONTROL_MAX_REQUEST_BYTES
            && max_request_bytes <= max_frame_bytes
            && max_response_bytes > 0
            && max_response_bytes <= CONTROL_MAX_RESPONSE_BYTES
            && max_response_bytes <= max_frame_bytes
            && max_json_depth > 0
            && max_json_depth <= CONTROL_MAX_JSON_DEPTH
            && max_string_bytes > 0
            && max_string_bytes <= CONTROL_MAX_STRING_BYTES;
        if !valid {
            return Err(CodecError::InvalidLimits);
        }
        Ok(Self {
            max_frame_bytes,
            max_request_bytes,
            max_response_bytes,
            max_json_depth,
            max_string_bytes,
        })
    }

    pub fn encode<T: Serialize>(
        &self,
        role: FrameRole,
        message: &T,
    ) -> Result<Vec<u8>, CodecError> {
        let limit = self.role_limit(role).min(self.max_frame_bytes);
        let mut writer = LimitedWriter::new(limit);
        serde_json::to_writer(&mut writer, message).map_err(|error| {
            if writer.limit_reached {
                CodecError::PayloadTooLarge {
                    declared: writer.attempted,
                    limit,
                }
            } else {
                CodecError::Json(error.to_string())
            }
        })?;
        let payload = writer.into_inner();
        self.validate_json_bytes(&payload)?;
        let payload_length =
            u32::try_from(payload.len()).map_err(|_| CodecError::PayloadTooLarge {
                declared: payload.len(),
                limit: u32::MAX as usize,
            })?;
        let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
        frame.extend_from_slice(&payload_length.to_be_bytes());
        frame.extend_from_slice(&payload);
        Ok(frame)
    }

    /// Private on purpose. A generic decode enforces only the shared byte and
    /// JSON bounds; the reserved-actor rejection and each envelope's own
    /// `validate()` live on the typed entry points below. While this was
    /// public, `read::<RequestEnvelope>` was a validation-free door to the same
    /// type `read_request` guards, sitting directly above it and looking like
    /// the general case. The typed entry points — the envelope readers and
    /// decoders, and the handshake readers — are the whole wire surface, so
    /// nothing is lost by making the bypass unreachable rather than merely
    /// discouraged.
    fn read<T: DeserializeOwned>(
        &self,
        role: FrameRole,
        reader: &mut impl Read,
    ) -> Result<T, CodecError> {
        let payload = self.read_payload(role, reader)?;
        self.decode_payload(&payload)
    }

    /// First client frame on a connection: the handshake, read under the
    /// pre-authentication ceiling of [`Self::handshake`]. Typed on purpose, like
    /// [`Self::read_request`]: the generic decoder is not a public door.
    pub fn read_handshake_request(
        &self,
        reader: &mut impl Read,
    ) -> Result<HandshakeRequest, CodecError> {
        self.read(FrameRole::Request, reader)
    }

    /// First server frame on a connection: the accepted handshake, or the
    /// redacted error the gateway sends before closing. It supersedes a plain
    /// `read_handshake_response`: a client that only knows how to read an
    /// acceptance cannot tell a refusal from a truncated frame, and the caller
    /// checks the acceptance against its own request with
    /// [`crate::handshake::HandshakeResponse::validate_for`] after
    /// [`HandshakeReply::into_accepted`].
    pub fn read_handshake_reply(
        &self,
        reader: &mut impl Read,
    ) -> Result<HandshakeReply, CodecError> {
        self.read(FrameRole::Response, reader)
    }

    pub fn read_request(&self, reader: &mut impl Read) -> Result<RequestEnvelope, CodecError> {
        let payload = self.read_payload(FrameRole::Request, reader)?;
        self.decode_request_payload(&payload)
    }

    pub fn read_response(&self, reader: &mut impl Read) -> Result<ResponseEnvelope, CodecError> {
        let response: ResponseEnvelope = self.read(FrameRole::Response, reader)?;
        response
            .validate()
            .map_err(|error| CodecError::Envelope(error.message))?;
        Ok(response)
    }

    /// Private for the same reason as [`Self::read`]: use
    /// [`Self::decode_request_frame`] or [`Self::decode_response_frame`].
    fn decode_frame<T: DeserializeOwned>(
        &self,
        role: FrameRole,
        frame: &[u8],
    ) -> Result<T, CodecError> {
        if frame.len() < FRAME_HEADER_BYTES {
            return Err(CodecError::TruncatedHeader);
        }
        let mut header = [0u8; FRAME_HEADER_BYTES];
        header.copy_from_slice(&frame[..FRAME_HEADER_BYTES]);
        let declared = u32::from_be_bytes(header) as usize;
        self.validate_declared(role, declared)?;
        let expected =
            FRAME_HEADER_BYTES
                .checked_add(declared)
                .ok_or(CodecError::PayloadTooLarge {
                    declared,
                    limit: self.role_limit(role),
                })?;
        if frame.len() < expected {
            return Err(CodecError::TruncatedPayload {
                declared,
                available: frame.len() - FRAME_HEADER_BYTES,
            });
        }
        if frame.len() > expected {
            return Err(CodecError::TrailingBytes(frame.len() - expected));
        }
        self.decode_payload(&frame[FRAME_HEADER_BYTES..])
    }

    pub fn decode_request_frame(&self, frame: &[u8]) -> Result<RequestEnvelope, CodecError> {
        if frame.len() < FRAME_HEADER_BYTES {
            return Err(CodecError::TruncatedHeader);
        }
        let mut header = [0u8; FRAME_HEADER_BYTES];
        header.copy_from_slice(&frame[..FRAME_HEADER_BYTES]);
        let declared = u32::from_be_bytes(header) as usize;
        self.validate_declared(FrameRole::Request, declared)?;
        let expected =
            FRAME_HEADER_BYTES
                .checked_add(declared)
                .ok_or(CodecError::PayloadTooLarge {
                    declared,
                    limit: self.role_limit(FrameRole::Request),
                })?;
        if frame.len() != expected {
            return if frame.len() < expected {
                Err(CodecError::TruncatedPayload {
                    declared,
                    available: frame.len() - FRAME_HEADER_BYTES,
                })
            } else {
                Err(CodecError::TrailingBytes(frame.len() - expected))
            };
        }
        self.decode_request_payload(&frame[FRAME_HEADER_BYTES..])
    }

    pub fn decode_response_frame(&self, frame: &[u8]) -> Result<ResponseEnvelope, CodecError> {
        let response: ResponseEnvelope = self.decode_frame(FrameRole::Response, frame)?;
        response
            .validate()
            .map_err(|error| CodecError::Envelope(error.message))?;
        Ok(response)
    }

    fn read_payload(&self, role: FrameRole, reader: &mut impl Read) -> Result<Vec<u8>, CodecError> {
        // The header is read byte-wise rather than with `read_exact` so a
        // read timeout can be told apart by *when* it fired: before any byte
        // of the next frame it is an idle connection, which a host keeps;
        // after the first byte it is a frame that stalled half-way, which a
        // host must not let hold a connection thread open.
        let mut header = [0u8; FRAME_HEADER_BYTES];
        let mut filled = 0usize;
        while filled < FRAME_HEADER_BYTES {
            match reader.read(&mut header[filled..]) {
                Ok(0) => return Err(CodecError::TruncatedHeader),
                Ok(read) => filled += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error)
                    if filled == 0
                        && matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                {
                    return Err(CodecError::IdleTimeout);
                }
                Err(error) => return Err(CodecError::Io(error.to_string())),
            }
        }
        let declared = u32::from_be_bytes(header) as usize;

        // This check intentionally precedes Vec allocation and every payload read.
        self.validate_declared(role, declared)?;

        let mut payload = vec![0u8; declared];
        let mut available = 0usize;
        while available < declared {
            match reader.read(&mut payload[available..]) {
                Ok(0) => {
                    return Err(CodecError::TruncatedPayload {
                        declared,
                        available,
                    });
                }
                Ok(read) => available += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(CodecError::Io(error.to_string())),
            }
        }
        Ok(payload)
    }

    fn validate_declared(&self, role: FrameRole, declared: usize) -> Result<(), CodecError> {
        let limit = self.role_limit(role).min(self.max_frame_bytes);
        if declared > limit {
            return Err(CodecError::PayloadTooLarge { declared, limit });
        }
        Ok(())
    }

    fn role_limit(&self, role: FrameRole) -> usize {
        match role {
            FrameRole::Request => self.max_request_bytes,
            FrameRole::Response => self.max_response_bytes,
        }
    }

    fn decode_payload<T: DeserializeOwned>(&self, payload: &[u8]) -> Result<T, CodecError> {
        self.validate_json_bytes(payload)?;
        serde_json::from_slice(payload).map_err(|error| CodecError::Json(error.to_string()))
    }

    fn decode_request_payload(&self, payload: &[u8]) -> Result<RequestEnvelope, CodecError> {
        self.validate_json_bytes(payload)?;
        let value: Value =
            serde_json::from_slice(payload).map_err(|error| CodecError::Json(error.to_string()))?;
        if value.as_object().is_some_and(|object| {
            RESERVED_ACTOR_FIELDS
                .iter()
                .any(|reserved| object.contains_key(*reserved))
        }) {
            return Err(CodecError::ReservedActorField);
        }
        let request: RequestEnvelope =
            serde_json::from_value(value).map_err(|error| CodecError::Json(error.to_string()))?;
        request
            .validate()
            .map_err(|error| CodecError::Envelope(error.message))?;
        Ok(request)
    }

    fn validate_json_bytes(&self, payload: &[u8]) -> Result<(), CodecError> {
        prescan_json(payload, self.max_json_depth, self.max_string_bytes)?;
        let value: Value =
            serde_json::from_slice(payload).map_err(|error| CodecError::Json(error.to_string()))?;
        validate_value(&value, 0, self.max_json_depth, self.max_string_bytes)
    }
}

impl TryFrom<&ProtocolLimits> for BoundedCodec {
    type Error = CodecError;

    fn try_from(limits: &ProtocolLimits) -> Result<Self, Self::Error> {
        limits.validate().map_err(|_| CodecError::InvalidLimits)?;
        Self::with_limits(
            limits.max_frame_bytes,
            limits.max_request_bytes,
            limits.max_response_bytes,
            limits.max_json_depth,
            limits.max_string_bytes,
        )
    }
}

fn prescan_json(
    payload: &[u8],
    max_depth: usize,
    max_string_bytes: usize,
) -> Result<(), CodecError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut string_bytes = 0usize;
    for byte in payload {
        if in_string {
            if escaped {
                escaped = false;
                string_bytes += 1;
            } else {
                match byte {
                    b'\\' => {
                        escaped = true;
                        string_bytes += 1;
                    }
                    b'"' => {
                        in_string = false;
                        string_bytes = 0;
                    }
                    _ => string_bytes += 1,
                }
            }
            if string_bytes > max_string_bytes {
                return Err(CodecError::StringTooLarge {
                    bytes: string_bytes,
                    limit: max_string_bytes,
                });
            }
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                string_bytes = 0;
            }
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(CodecError::JsonTooDeep { depth, max_depth });
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn validate_value(
    value: &Value,
    depth: usize,
    max_depth: usize,
    max_string_bytes: usize,
) -> Result<(), CodecError> {
    if depth > max_depth {
        return Err(CodecError::JsonTooDeep { depth, max_depth });
    }
    match value {
        Value::Number(number) if number.is_f64() => return Err(CodecError::FloatingPointNumber),
        Value::String(value) if value.len() > max_string_bytes => {
            return Err(CodecError::StringTooLarge {
                bytes: value.len(),
                limit: max_string_bytes,
            });
        }
        Value::Array(values) => {
            for value in values {
                validate_value(value, depth + 1, max_depth, max_string_bytes)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key.len() > max_string_bytes {
                    return Err(CodecError::StringTooLarge {
                        bytes: key.len(),
                        limit: max_string_bytes,
                    });
                }
                validate_value(value, depth + 1, max_depth, max_string_bytes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    attempted: usize,
    limit_reached: bool,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            attempted: 0,
            limit_reached: false,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.attempted = self.bytes.len().saturating_add(buffer.len());
        if self.attempted > self.limit {
            self.limit_reached = true;
            return Err(io::Error::other("encoded control frame exceeds its limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    InvalidLimits,
    /// The reader's timeout elapsed before any byte of the next frame arrived:
    /// an idle peer, not a broken frame. Only a reader with a timeout set can
    /// observe this.
    IdleTimeout,
    TruncatedHeader,
    PayloadTooLarge {
        declared: usize,
        limit: usize,
    },
    TruncatedPayload {
        declared: usize,
        available: usize,
    },
    TrailingBytes(usize),
    JsonTooDeep {
        depth: usize,
        max_depth: usize,
    },
    StringTooLarge {
        bytes: usize,
        limit: usize,
    },
    FloatingPointNumber,
    ReservedActorField,
    Json(String),
    Envelope(String),
    Io(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdleTimeout => formatter.write_str("no frame arrived before the read timeout"),
            Self::InvalidLimits => {
                formatter.write_str("codec limits exceed the reviewed control contract")
            }
            Self::TruncatedHeader => formatter.write_str("control frame header is truncated"),
            Self::PayloadTooLarge { declared, limit } => {
                write!(
                    formatter,
                    "declared payload is {declared} bytes; limit is {limit}"
                )
            }
            Self::TruncatedPayload {
                declared,
                available,
            } => write!(
                formatter,
                "declared payload is {declared} bytes but only {available} are available"
            ),
            Self::TrailingBytes(bytes) => write!(formatter, "frame has {bytes} trailing bytes"),
            Self::JsonTooDeep { depth, max_depth } => {
                write!(formatter, "JSON depth {depth} exceeds limit {max_depth}")
            }
            Self::StringTooLarge { bytes, limit } => {
                write!(formatter, "JSON string is {bytes} bytes; limit is {limit}")
            }
            Self::FloatingPointNumber => {
                formatter.write_str("floating-point JSON numbers are forbidden")
            }
            Self::ReservedActorField => {
                formatter.write_str("external request contains a reserved actor field")
            }
            Self::Json(message) => write!(formatter, "invalid JSON: {message}"),
            Self::Envelope(message) => write!(formatter, "invalid envelope: {message}"),
            Self::Io(message) => write!(formatter, "frame I/O failed: {message}"),
        }
    }
}

impl std::error::Error for CodecError {}

#[cfg(test)]
mod tests {
    use super::*;

    struct HeaderThenPanic {
        header: io::Cursor<[u8; FRAME_HEADER_BYTES]>,
        payload_read_attempted: bool,
    }

    impl Read for HeaderThenPanic {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.header.read(buffer)?;
            if read == 0 {
                self.payload_read_attempted = true;
                panic!("codec read payload after rejecting its declared length");
            }
            Ok(read)
        }
    }

    #[test]
    fn oversized_header_is_rejected_before_payload_read_or_allocation() {
        let codec = BoundedCodec::with_limits(32, 16, 32, 8, 16).unwrap();
        let mut reader = HeaderThenPanic {
            header: io::Cursor::new(17u32.to_be_bytes()),
            payload_read_attempted: false,
        };
        let result = codec.read::<Value>(FrameRole::Request, &mut reader);
        assert_eq!(
            result,
            Err(CodecError::PayloadTooLarge {
                declared: 17,
                limit: 16
            })
        );
        assert!(!reader.payload_read_attempted);
    }

    #[test]
    fn pre_authentication_codec_uses_the_smaller_handshake_ceiling() {
        let codec = BoundedCodec::handshake();
        let declared = u32::try_from(CONTROL_HANDSHAKE_MAX_BYTES + 1).unwrap();
        let mut reader = HeaderThenPanic {
            header: io::Cursor::new(declared.to_be_bytes()),
            payload_read_attempted: false,
        };
        assert_eq!(
            codec.read::<Value>(FrameRole::Request, &mut reader),
            Err(CodecError::PayloadTooLarge {
                declared: CONTROL_HANDSHAKE_MAX_BYTES + 1,
                limit: CONTROL_HANDSHAKE_MAX_BYTES,
            })
        );
        assert!(!reader.payload_read_attempted);
    }

    #[test]
    fn codec_rejects_floats_and_excess_depth() {
        let codec = BoundedCodec::with_limits(128, 128, 128, 2, 32).unwrap();
        assert_eq!(
            codec.encode(FrameRole::Request, &serde_json::json!(1.5)),
            Err(CodecError::FloatingPointNumber)
        );
        assert!(matches!(
            codec.encode(FrameRole::Request, &serde_json::json!({"a": {"b": {}}})),
            Err(CodecError::JsonTooDeep { .. })
        ));
        let short_strings = BoundedCodec::with_limits(128, 128, 128, 2, 4).unwrap();
        assert!(matches!(
            short_strings.encode(FrameRole::Request, &serde_json::json!("12345")),
            Err(CodecError::StringTooLarge { .. })
        ));
    }

    #[test]
    fn custom_limits_cannot_raise_a_reviewed_hard_bound() {
        assert_eq!(
            BoundedCodec::with_limits(
                CONTROL_PROTOCOL_MAX_FRAME_BYTES + 1,
                CONTROL_MAX_REQUEST_BYTES,
                CONTROL_MAX_RESPONSE_BYTES,
                CONTROL_MAX_JSON_DEPTH,
                CONTROL_MAX_STRING_BYTES,
            )
            .unwrap_err(),
            CodecError::InvalidLimits
        );
    }
}
