//! Who may speak for the terminal: the per-run secret the execution bridge
//! proves before quantick reads a line from it or writes an order to it.
//!
//! Loopback alone does not name a process: any local program can dial the
//! listener and, once it is the connected bridge, show the venue a false
//! account and receive every order. Each run therefore draws a random secret
//! that quantick publishes in a file only the current user can read; the
//! bridge reads that file and sends the secret, hex-encoded, as its first
//! line. A connection that sends anything else, sends nothing, or dribbles
//! the line past [`AUTH_DEADLINE`] is closed before it becomes the bridge.

use std::io::{self, BufRead, BufReader};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;

/// Bytes of randomness in one run's secret.
pub const BRIDGE_SECRET_BYTES: usize = 32;

/// How long a connection has to send the whole first line. A bridge on the
/// same machine sends it at once; the bound stops a connection that sends
/// nothing, or one byte at a time, from holding the one bridge slot.
pub const AUTH_DEADLINE: Duration = Duration::from_secs(2);

/// Longest first line read: the hex secret is 64 bytes, plus the line end.
const MAX_AUTH_LINE_BYTES: usize = 128;

/// One run's secret. Its value never reaches a log: `Debug` shows nothing.
#[derive(Clone)]
pub struct BridgeSecret([u8; BRIDGE_SECRET_BYTES]);

impl std::fmt::Debug for BridgeSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BridgeSecret(<redacted>)")
    }
}

impl BridgeSecret {
    /// Draw a fresh secret from the operating system's random source.
    ///
    /// # Errors
    ///
    /// The random source is unavailable; there is no weaker fallback.
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; BRIDGE_SECRET_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self(bytes))
    }

    /// A known secret, for tests that play the bridge.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; BRIDGE_SECRET_BYTES]) -> Self {
        Self(bytes)
    }

    /// The lowercase hex the secret file holds and the bridge sends.
    #[must_use]
    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(BRIDGE_SECRET_BYTES * 2);
        for byte in self.0 {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        text
    }

    /// Whether `line` (its line end already removed) is this secret's hex,
    /// compared in constant time. The length is public, so a line of the
    /// wrong length fails without a comparison.
    fn matches(&self, line: &[u8]) -> bool {
        bool::from(line.ct_eq(self.to_hex().as_bytes()))
    }
}

/// Read the first line from `reader` within [`AUTH_DEADLINE`] and check it
/// against `secret`. On `true` the reader holds whatever the bridge sent
/// after the line, and the socket's read timeout is cleared again; on
/// `false` the caller drops the connection.
pub(crate) fn authenticate(reader: &mut BufReader<TcpStream>, secret: &BridgeSecret) -> bool {
    let started = Instant::now();
    let mut line = Vec::with_capacity(MAX_AUTH_LINE_BYTES);
    loop {
        let Some(left) = AUTH_DEADLINE.checked_sub(started.elapsed()) else {
            return false;
        };
        if left.is_zero() || reader.get_ref().set_read_timeout(Some(left)).is_err() {
            return false;
        }
        let available = match reader.fill_buf() {
            Ok([]) | Err(_) => return false,
            Ok(available) => available,
        };
        let (taken, ended) = match available.iter().position(|&byte| byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        if line.len() + taken > MAX_AUTH_LINE_BYTES {
            return false;
        }
        line.extend_from_slice(&available[..taken]);
        reader.consume(taken);
        if ended {
            break;
        }
    }
    if reader.get_ref().set_read_timeout(None).is_err() {
        return false;
    }
    let text = line.trim_ascii();
    secret.matches(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase_and_two_digits_a_byte() {
        let mut bytes = [0u8; BRIDGE_SECRET_BYTES];
        bytes[0] = 0x0a;
        bytes[1] = 0xf0;
        let hex = BridgeSecret::from_bytes(bytes).to_hex();
        assert_eq!(hex.len(), BRIDGE_SECRET_BYTES * 2);
        assert!(hex.starts_with("0af000"), "{hex}");
    }

    #[test]
    fn only_the_exact_hex_matches() {
        let secret = BridgeSecret::from_bytes([0xab; BRIDGE_SECRET_BYTES]);
        let hex = secret.to_hex();
        assert!(secret.matches(hex.as_bytes()));
        assert!(!secret.matches(hex.to_uppercase().as_bytes()));
        assert!(!secret.matches(&hex.as_bytes()[1..]));
        assert!(!secret.matches(b""));
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let secret = BridgeSecret::from_bytes([7; BRIDGE_SECRET_BYTES]);
        assert!(!format!("{secret:?}").contains(&secret.to_hex()));
    }

    #[test]
    fn two_draws_differ() {
        let first = BridgeSecret::generate().expect("random source");
        let second = BridgeSecret::generate().expect("random source");
        assert_ne!(first.to_hex(), second.to_hex());
    }
}
