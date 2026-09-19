//! Drawing keyboard cancellation order. The caller performs each attempted
//! transition and reports whether it consumed Escape; no guessed paper facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeLayer {
    Rail,
    Paper,
    Confirmation,
    QuickRange,
    InlineText,
    Draft,
    Selection,
    Pointer,
}

pub fn consume_escape(mut attempt: impl FnMut(EscapeLayer) -> bool) {
    use EscapeLayer::*;
    for layer in [
        Rail,
        Paper,
        Confirmation,
        QuickRange,
        InlineText,
        Draft,
        Selection,
        Pointer,
    ] {
        if attempt(layer) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paper_consumption_prevents_every_drawing_cancellation() {
        let mut visited = Vec::new();
        consume_escape(|layer| {
            visited.push(layer);
            layer == EscapeLayer::Paper
        });
        assert_eq!(visited, [EscapeLayer::Rail, EscapeLayer::Paper]);
    }
    #[test]
    fn absent_gestures_reach_pointer_after_the_inline_editor() {
        let mut visited = Vec::new();
        consume_escape(|layer| {
            visited.push(layer);
            false
        });
        assert_eq!(
            visited,
            [
                EscapeLayer::Rail,
                EscapeLayer::Paper,
                EscapeLayer::Confirmation,
                EscapeLayer::QuickRange,
                EscapeLayer::InlineText,
                EscapeLayer::Draft,
                EscapeLayer::Selection,
                EscapeLayer::Pointer
            ]
        );
    }
}
