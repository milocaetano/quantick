//! Properties of the control frame codec over untrusted bytes: no input makes
//! a decoder panic, a declared length past the limit is refused before any
//! payload is read, an accepted frame stays within the codec's depth and
//! string bounds, and every valid envelope survives encode then decode.

use std::io::Cursor;

use proptest::prelude::*;
use quantick_control::{
    codec::{BoundedCodec, CodecError, FrameRole},
    fake::COUNTER_READ,
    id::{CapabilityId, InstanceId, RequestId},
    limits::{CONTROL_HANDSHAKE_MAX_BYTES, CONTROL_MAX_REQUEST_BYTES, CONTROL_MAX_RESPONSE_BYTES},
    wire::{RequestEnvelope, ResponseEnvelope, ResponseOutcome},
};
use serde_json::{Map, Value};

/// A codec small enough that generated values cross its depth and string
/// bounds often. Strings must still fit the envelope's own identifiers.
const SMALL_DEPTH: usize = 8;
const SMALL_STRING_BYTES: usize = 96;
const SMALL_FRAME_BYTES: usize = 16 * 1024;

fn small_codec() -> BoundedCodec {
    BoundedCodec::with_limits(
        SMALL_FRAME_BYTES,
        SMALL_FRAME_BYTES,
        SMALL_FRAME_BYTES,
        SMALL_DEPTH,
        SMALL_STRING_BYTES,
    )
    .unwrap()
}

fn codecs() -> [BoundedCodec; 3] {
    [
        BoundedCodec::default(),
        BoundedCodec::handshake(),
        small_codec(),
    ]
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut frame = u32::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(payload);
    frame
}

/// Every public decoding door, fed the same bytes; none may panic.
fn decode_everywhere(codec: &BoundedCodec, bytes: &[u8]) {
    let _ = codec.decode_request_frame(bytes);
    let _ = codec.decode_response_frame(bytes);
    let _ = codec.read_request(&mut Cursor::new(bytes));
    let _ = codec.read_response(&mut Cursor::new(bytes));
    let _ = codec.read_handshake_request(&mut Cursor::new(bytes));
    let _ = codec.read_handshake_reply(&mut Cursor::new(bytes));
    let _ = codec.read_handshake_response(&mut Cursor::new(bytes));
}

/// JSON values without floats, which the control plane refuses by contract.
fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        "\\PC{0,24}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map("\\PC{0,12}", inner, 0..6)
                .prop_map(|map| Value::Object(map.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// Text that is mostly JSON punctuation and envelope field names, so the
/// decoders get past the first byte far more often than random bytes do.
fn json_like_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("{".to_owned()),
        Just("}".to_owned()),
        Just("[".to_owned()),
        Just("]".to_owned()),
        Just("\"".to_owned()),
        Just("\\".to_owned()),
        Just(":".to_owned()),
        Just(",".to_owned()),
        Just("1.5".to_owned()),
        Just("-0".to_owned()),
        Just("1e400".to_owned()),
        Just("18446744073709551616".to_owned()),
        Just("null".to_owned()),
        Just("true".to_owned()),
        Just("\"protocol_version\":1".to_owned()),
        Just("\"request_id\":\"r\"".to_owned()),
        Just("\"instance_id\":\"AQEBAQEBAQEBAQEBAQEBAQ\"".to_owned()),
        Just("\"capability_id\":\"fake.counter.read\"".to_owned()),
        Just("\"capability_version\":1".to_owned()),
        Just("\"payload\":".to_owned()),
        Just("\"result\":".to_owned()),
        Just("\"error\":".to_owned()),
        Just("\"principal_id\":\"x\"".to_owned()),
        "\\PC{0,4}",
    ];
    prop::collection::vec(piece, 0..48).prop_map(|pieces| pieces.concat())
}

/// How many containers nest at the deepest point, the unit both the byte
/// pre-scan and the value walk bound.
fn nesting(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(nesting).max().unwrap_or(0),
        Value::Object(values) => 1 + values.values().map(nesting).max().unwrap_or(0),
        _ => 0,
    }
}

/// The longest string or object key anywhere in `value`, in decoded bytes.
fn longest_string(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len(),
        Value::Array(values) => values.iter().map(longest_string).max().unwrap_or(0),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| key.len().max(longest_string(value)))
            .max()
            .unwrap_or(0),
        _ => 0,
    }
}

fn nested_in_arrays(mut value: Value, levels: usize) -> Value {
    for _ in 0..levels {
        value = Value::Array(vec![value]);
    }
    value
}

fn request(
    protocol_version: u32,
    request_id: String,
    instance: [u8; 16],
    dry_run: bool,
    reason: Option<String>,
    payload: Value,
) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version,
        request_id: RequestId::new(request_id).unwrap(),
        instance_id: InstanceId::from_bytes(instance),
        capability_id: CapabilityId::new(COUNTER_READ).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run,
        reason,
        payload,
    }
}

fn request_strategy() -> impl Strategy<Value = RequestEnvelope> {
    (
        0_u32..=3,
        "[ -~]{1,32}",
        any::<[u8; 16]>(),
        any::<bool>(),
        prop::option::of("\\PC{0,48}"),
        json_value(),
    )
        .prop_map(|(version, id, instance, dry_run, reason, payload)| {
            request(version, id, instance, dry_run, reason, payload)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic_a_decoder(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        for codec in codecs() {
            decode_everywhere(&codec, &bytes);
            // The same bytes behind an honest header reach the JSON stage.
            decode_everywhere(&codec, &framed(&bytes));
        }
    }

    #[test]
    fn json_like_frames_never_panic_a_decoder(text in json_like_text()) {
        let frame = framed(text.as_bytes());
        for codec in codecs() {
            decode_everywhere(&codec, &frame);
        }
    }

    #[test]
    fn a_declared_length_past_the_limit_is_refused_before_the_payload(
        excess in 1_u32..=u32::MAX - u32::try_from(CONTROL_MAX_RESPONSE_BYTES).unwrap(),
        role in prop_oneof![Just(FrameRole::Request), Just(FrameRole::Response)],
    ) {
        for (codec, limit) in [
            (
                BoundedCodec::default(),
                match role {
                    FrameRole::Request => CONTROL_MAX_REQUEST_BYTES,
                    FrameRole::Response => CONTROL_MAX_RESPONSE_BYTES,
                },
            ),
            (BoundedCodec::handshake(), CONTROL_HANDSHAKE_MAX_BYTES),
            (small_codec(), SMALL_FRAME_BYTES),
        ] {
            let declared = u32::try_from(limit).unwrap() + excess;
            // Only the header: a decoder that read or allocated the payload
            // first would report a truncation instead.
            let header = declared.to_be_bytes();
            let refused: Result<(), CodecError> = Err(CodecError::PayloadTooLarge {
                declared: declared as usize,
                limit,
            });
            let (decoded, read) = match role {
                FrameRole::Request => (
                    codec.decode_request_frame(&header).map(|_| ()),
                    codec.read_request(&mut Cursor::new(header)).map(|_| ()),
                ),
                FrameRole::Response => (
                    codec.decode_response_frame(&header).map(|_| ()),
                    codec.read_response(&mut Cursor::new(header)).map(|_| ()),
                ),
            };
            prop_assert_eq!(decoded, refused.clone());
            prop_assert_eq!(read, refused);
        }
    }

    #[test]
    fn an_accepted_frame_stays_within_the_depth_and_string_bounds(
        envelope in request_strategy(),
        extra_levels in 0_usize..12,
        long_key in prop::option::of("[a-z]{90,140}"),
    ) {
        let mut envelope = envelope;
        let mut payload = nested_in_arrays(envelope.payload.clone(), extra_levels);
        if let Some(key) = long_key {
            let mut object = Map::new();
            object.insert(key, payload);
            payload = Value::Object(object);
        }
        envelope.payload = payload;
        let Ok(frame) = BoundedCodec::default().encode(FrameRole::Request, &envelope) else {
            return Ok(());
        };
        let value: Value = serde_json::from_slice(&frame[4..]).unwrap();
        let codec = small_codec();
        match codec.decode_request_frame(&frame) {
            Ok(decoded) => {
                let decoded = serde_json::to_value(decoded).unwrap();
                prop_assert!(nesting(&decoded) <= SMALL_DEPTH);
                prop_assert!(longest_string(&decoded) <= SMALL_STRING_BYTES);
                prop_assert!(frame.len() - 4 <= SMALL_FRAME_BYTES);
            }
            Err(error) => {
                // Each bound the frame breaks must be one the codec reports;
                // it is only free to report another failure first.
                if nesting(&value) > SMALL_DEPTH && longest_string(&value) <= SMALL_STRING_BYTES {
                    prop_assert!(
                        matches!(
                            error,
                            CodecError::JsonTooDeep { .. }
                                // The pre-scan counts a string's escaped bytes.
                                | CodecError::StringTooLarge { .. }
                                | CodecError::PayloadTooLarge { .. }
                        ),
                        "{error:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_request_survives_encode_then_decode(envelope in request_strategy()) {
        let codec = BoundedCodec::default();
        let frame = codec.encode(FrameRole::Request, &envelope).unwrap();
        let decoded = codec.decode_request_frame(&frame);
        if envelope.validate().is_ok() {
            prop_assert_eq!(decoded, Ok(envelope.clone()));
            prop_assert_eq!(codec.read_request(&mut Cursor::new(&frame)), Ok(envelope));
        } else {
            prop_assert!(decoded.is_err());
        }
    }

    #[test]
    fn a_success_response_survives_encode_then_decode(
        request_id in "[ -~]{1,32}",
        instance in any::<[u8; 16]>(),
        result in json_value(),
    ) {
        let response = ResponseEnvelope {
            protocol_version: 1,
            request_id: RequestId::new(request_id).unwrap(),
            instance_id: InstanceId::from_bytes(instance),
            capture_revision: None,
            module_revisions: Vec::new(),
            outcome: ResponseOutcome::Success { result },
            warnings: Vec::new(),
        };
        let codec = BoundedCodec::default();
        let frame = codec.encode(FrameRole::Response, &response).unwrap();
        prop_assert_eq!(codec.decode_response_frame(&frame).unwrap(), response.clone());
        prop_assert_eq!(codec.read_response(&mut Cursor::new(&frame)).unwrap(), response);
    }
}
