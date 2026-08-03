//! Pure decoding for `l2Book` WebSocket envelopes.

use serde::Deserialize;

/// One resting level in a complete Hyperliquid book image.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawLevel {
    /// Price.
    #[serde(rename = "px")]
    pub price: String,
    /// Aggregate size resting at that price.
    #[serde(rename = "sz")]
    pub size: String,
    /// Number of orders represented by the level.
    pub n: u64,
}

/// Complete top-of-book image: `[bids, asks]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookImage {
    /// Perpetual coin name.
    pub coin: String,
    /// Venue event time in epoch milliseconds.
    pub time: i64,
    /// At most 20 levels on each side.
    pub levels: [Vec<RawLevel>; 2],
}

/// A decoded frame on an L2 subscription socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookMessage {
    /// Subscription acknowledgement.
    Subscribed,
    /// Application-level heartbeat response.
    Pong,
    /// Complete authoritative visible book image.
    Book(BookImage),
    /// Valid envelope from an unrequested channel.
    Other { channel: String },
}

#[derive(Deserialize)]
struct Envelope {
    channel: String,
    #[serde(default)]
    data: serde_json::Value,
}

/// Decode one L2 WebSocket envelope.
///
/// # Errors
///
/// Returns a serde error if the envelope or `l2Book` image is malformed.
pub fn parse_book_message(json: &str) -> Result<BookMessage, serde_json::Error> {
    let envelope: Envelope = serde_json::from_str(json)?;
    match envelope.channel.as_str() {
        "subscriptionResponse" => Ok(BookMessage::Subscribed),
        "pong" => Ok(BookMessage::Pong),
        "l2Book" => serde_json::from_value(envelope.data).map(BookMessage::Book),
        _ => Ok(BookMessage::Other {
            channel: envelope.channel,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_with_extra_fields_is_accepted() {
        let text = r#"{"channel":"subscriptionResponse","data":{"method":"subscribe","subscription":{"type":"l2Book","coin":"BTC","nSigFigs":null}}}"#;
        assert_eq!(parse_book_message(text).unwrap(), BookMessage::Subscribed);
    }
}
