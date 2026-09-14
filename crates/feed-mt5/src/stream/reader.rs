//! A line reader that refuses to buffer without bound.
//!
//! Generic over its source so the classification decisions above it can be
//! tested against bytes rather than against a live socket.

use tokio::io::{AsyncBufReadExt as _, BufReader};

use super::MAX_LINE_BYTES;

#[cfg(test)]
mod bounded_reader_tests {
    use super::{BoundedLine, BoundedLineReader, MAX_LINE_BYTES};

    /// The reader is generic so the app can point it at a launched bridge's
    /// stderr, not just a socket. A cursor stands in for both.
    #[tokio::test]
    async fn it_reads_lines_from_any_source_not_just_a_socket() {
        let source = std::io::Cursor::new(b"first\nsecond\r\n".to_vec());
        let mut reader = BoundedLineReader::new(source);
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("first".to_owned())
        );
        // A CRLF terminator loses the carriage return, like the socket path.
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("second".to_owned())
        );
        assert_eq!(reader.next_line().await.unwrap(), BoundedLine::Eof);
    }

    #[tokio::test]
    async fn an_endless_line_is_capped_and_reading_continues() {
        // The failure this bound exists for: output with no newline in it.
        // The oversized line is dropped, and the line after it still arrives.
        let mut bytes = vec![b'x'; MAX_LINE_BYTES + 10];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"survivor\n");
        let mut reader = BoundedLineReader::new(std::io::Cursor::new(bytes));
        assert_eq!(reader.next_line().await.unwrap(), BoundedLine::TooLong);
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("survivor".to_owned())
        );
    }

    #[tokio::test]
    async fn invalid_utf8_is_reported_and_skipped_not_fatal() {
        // A Windows-encoded path inside a Python traceback is exactly this.
        let mut bytes = b"before\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
        bytes.extend_from_slice(b"after\n");
        let mut reader = BoundedLineReader::new(std::io::Cursor::new(bytes));
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("before".to_owned())
        );
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::NotUtf8 { len: 2 }
        );
        assert_eq!(
            reader.next_line().await.unwrap(),
            BoundedLine::Line("after".to_owned()),
            "one unreadable line must not end the stream"
        );
    }
}

/// One read from a [`BoundedLineReader`].
#[derive(Debug, PartialEq, Eq)]
pub enum BoundedLine {
    /// A complete UTF-8 line (terminator stripped).
    Line(String),
    /// A complete line that was not valid UTF-8; skippable, per PROTOCOL.md.
    NotUtf8 {
        /// Length of the rejected line, in bytes.
        len: usize,
    },
    /// The peer streamed more than [`MAX_LINE_BYTES`] without a newline.
    TooLong,
    /// The peer closed the connection.
    Eof,
}

/// What one `fill_buf` round decided (split out so the borrow of the reader's
/// internal buffer ends before `consume`).
enum ReadStep {
    Eof,
    Line(usize),
    TooLong(usize),
    More(usize),
}

/// A newline-delimited reader that never buffers more than
/// [`MAX_LINE_BYTES`], unlike `AsyncBufReadExt::lines`. Cancel-safe: the only
/// await is `fill_buf`, and bytes move out of the source buffer and into the
/// line buffer within a single poll.
///
/// Generic over the source because the bridge speaks the same line-delimited
/// shape over two transports: a socket, and the stderr of a bridge quantick
/// launched itself. Both need the same bound — an unterminated line is a
/// memory leak wherever it comes from — and one implementation is what keeps
/// the two honest about it.
pub struct BoundedLineReader<R> {
    reader: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: tokio::io::AsyncRead + Unpin> BoundedLineReader<R> {
    /// Wrap `source`, reading at most [`MAX_LINE_BYTES`] per line.
    pub fn new(source: R) -> Self {
        Self {
            reader: BufReader::new(source),
            buf: Vec::new(),
        }
    }

    /// The next line, or what went wrong with it.
    pub async fn next_line(&mut self) -> std::io::Result<BoundedLine> {
        loop {
            let step = {
                let available = self.reader.fill_buf().await?;
                if available.is_empty() {
                    ReadStep::Eof
                } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                    if self.buf.len() + pos > MAX_LINE_BYTES {
                        ReadStep::TooLong(pos + 1)
                    } else {
                        self.buf.extend_from_slice(&available[..pos]);
                        ReadStep::Line(pos + 1)
                    }
                } else if self.buf.len() + available.len() > MAX_LINE_BYTES {
                    ReadStep::TooLong(available.len())
                } else {
                    self.buf.extend_from_slice(available);
                    ReadStep::More(available.len())
                }
            };
            match step {
                ReadStep::Eof => {
                    // A trailing unterminated line still counts, like
                    // `lines()` behaves.
                    if self.buf.is_empty() {
                        return Ok(BoundedLine::Eof);
                    }
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::Line(consume) => {
                    self.reader.consume(consume);
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::TooLong(consume) => {
                    self.reader.consume(consume);
                    self.buf.clear();
                    return Ok(BoundedLine::TooLong);
                }
                ReadStep::More(consume) => self.reader.consume(consume),
            }
        }
    }

    fn finish(mut line: Vec<u8>) -> BoundedLine {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        match String::from_utf8(line) {
            Ok(text) => BoundedLine::Line(text),
            Err(e) => BoundedLine::NotUtf8 {
                len: e.as_bytes().len(),
            },
        }
    }
}
