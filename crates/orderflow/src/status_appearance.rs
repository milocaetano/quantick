//! Semantic status ink; the renderer converts the selected appearance token.
use crate::engine::CaptureStatus;
use quantick_appearance::Token;

pub fn status_token(status: &CaptureStatus) -> Token {
    match status {
        CaptureStatus::Live { .. } => Token::Buy,
        CaptureStatus::Connecting | CaptureStatus::Buffering | CaptureStatus::SnapshotFetching => {
            Token::Amber
        }
        CaptureStatus::Disabled => Token::TextMuted,
        CaptureStatus::Resyncing { .. }
        | CaptureStatus::Disconnected { .. }
        | CaptureStatus::Error => Token::Warn,
    }
}
